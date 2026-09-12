use std::collections::HashMap;

use serde_json::Value;

use crate::command_planner::SemanticOperation;
use crate::model::{Document, Fragment, Mark, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::transform::apply_step_canonical_marks;
use crate::yrs_engine::{OperationError, OperationResult, StructuralEdit, StructuralEditBatch};

const BATCH_FIELD: &str = "structure";
const BATCH_OPERATION_INDEX: usize = 0;
const ROOT_PATH: &[u32] = &[];
const FIRST_CHILD: u32 = 0;
const NO_CHILDREN: usize = 0;

enum ChildOrigin {
    Base(u32),
    Created(Node),
}

struct ParentPlan {
    entries: Vec<ChildOrigin>,
}

enum LocatedTarget {
    Base(Vec<u32>),
    Created {
        parent: Vec<u32>,
        entry: usize,
        relative: Vec<u32>,
    },
}

struct PendingText {
    parent_path: Vec<u32>,
    parent_offset: u32,
    text: String,
    marks: Vec<Mark>,
}

struct BatchBuilder<'a> {
    request_id: u64,
    base: &'a Document,
    schema: &'a Schema,
    shadow: Document,
    plans: HashMap<Vec<u32>, ParentPlan>,
    patches: Vec<(Vec<u32>, HashMap<String, Value>)>,
    texts: Vec<PendingText>,
}

pub(super) fn structural_edit_batch(
    request_id: u64,
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
    selection_after: &Selection,
) -> OperationResult<Option<StructuralEditBatch>> {
    let mut builder = BatchBuilder {
        request_id,
        base: document,
        schema,
        shadow: document.clone(),
        plans: HashMap::new(),
        patches: Vec::new(),
        texts: Vec::new(),
    };
    for operation in operations {
        if builder.absorb(operation)?.is_none() {
            return Ok(None);
        }
    }
    let Some(edits) = builder.into_edits() else {
        return Ok(None);
    };
    Ok((!edits.is_empty()).then(|| StructuralEditBatch::new(edits, selection_after.clone())))
}

fn child_index_at_offset(parent: &Node, offset: u32) -> Option<u32> {
    let content = parent.content()?;
    let mut cursor = 0u32;
    for (index, child) in content.iter().enumerate() {
        if cursor == offset {
            return u32::try_from(index).ok();
        }
        if child.is_text() {
            return None;
        }
        cursor = cursor.checked_add(child.node_size())?;
    }
    (cursor == offset).then(|| u32::try_from(content.child_count()).ok())?
}

fn rebuild_at(
    node: &Node,
    relative: &[u32],
    apply: &dyn Fn(&Node) -> Option<Node>,
) -> Option<Node> {
    let Some((index, rest)) = relative.split_first() else {
        return apply(node);
    };
    let content = node.content()?;
    let index = usize::try_from(*index).ok()?;
    let child = content.child(index)?;
    let rebuilt = rebuild_at(child, rest, apply)?;
    let mut children = content.children().to_vec();
    *children.get_mut(index)? = rebuilt;
    (!node.is_void() && !node.is_text()).then(|| {
        Node::element(
            node.node_type().to_owned(),
            node.attrs().clone(),
            Fragment::from(children),
        )
    })
}

impl BatchBuilder<'_> {
    fn absorb(&mut self, operation: &SemanticOperation) -> OperationResult<Option<()>> {
        let absorbed = match operation {
            SemanticOperation::ReplaceRange { from, to, content } => {
                self.absorb_splice(*from, *to, content)
            }
            SemanticOperation::UpdateNodeAttrs { pos, attrs } => self.absorb_patch(*pos, attrs),
            SemanticOperation::InsertText { pos, text, marks } => {
                self.absorb_text(*pos, text, marks)
            }
            SemanticOperation::DeleteRange { .. }
            | SemanticOperation::AddMark { .. }
            | SemanticOperation::RemoveMark { .. }
            | SemanticOperation::ReplaceMark { .. }
            | SemanticOperation::SplitBlock { .. }
            | SemanticOperation::JoinBlocks { .. }
            | SemanticOperation::UnwrapFromList { .. }
            | SemanticOperation::OutdentListItem { .. }
            | SemanticOperation::WrapInList { .. }
            | SemanticOperation::IndentListItem { .. }
            | SemanticOperation::InsertNode { .. } => None,
        };
        if absorbed.is_none() {
            return Ok(None);
        }
        let (next, _) = apply_step_canonical_marks(&self.shadow, &operation.as_step(), self.schema)
            .map_err(|error| {
                OperationError::operation_invalid(
                    self.request_id,
                    BATCH_OPERATION_INDEX,
                    BATCH_FIELD,
                    error.to_string(),
                )
            })?;
        self.shadow = next;
        Ok(Some(()))
    }

    fn locate(&self, shadow_path: &[u32]) -> Option<LocatedTarget> {
        let mut base_path: Vec<u32> = Vec::new();
        for (depth, index) in shadow_path.iter().copied().enumerate() {
            let Some(plan) = self.plans.get(&base_path) else {
                base_path.push(index);
                continue;
            };
            let entry = usize::try_from(index).ok()?;
            match plan.entries.get(entry)? {
                ChildOrigin::Base(base_index) => base_path.push(*base_index),
                ChildOrigin::Created(_) => {
                    return Some(LocatedTarget::Created {
                        parent: base_path,
                        entry,
                        relative: shadow_path
                            .get(depth.checked_add(1)?..)
                            .unwrap_or_default()
                            .to_vec(),
                    })
                }
            }
        }
        Some(LocatedTarget::Base(base_path))
    }

    fn plan_for(&mut self, base_parent: &[u32]) -> Option<&mut ParentPlan> {
        if !self.plans.contains_key(base_parent) {
            let children = self.base.node_at(base_parent)?.content()?.child_count();
            let entries = (0..u32::try_from(children).ok()?)
                .map(ChildOrigin::Base)
                .collect();
            self.plans
                .insert(base_parent.to_vec(), ParentPlan { entries });
        }
        self.plans.get_mut(base_parent)
    }

    fn forget_subtree(&mut self, deleted: &[u32]) {
        self.plans.retain(|path, _| !path.starts_with(deleted));
        self.patches.retain(|(path, _)| !path.starts_with(deleted));
        self.texts
            .retain(|text| !text.parent_path.starts_with(deleted));
    }

    fn absorb_splice(&mut self, from: u32, to: u32, content: &Fragment) -> Option<()> {
        let from_resolved = self.shadow.resolve(from).ok()?;
        let to_resolved = self.shadow.resolve(to).ok()?;
        if from_resolved.node_path != to_resolved.node_path {
            return None;
        }
        let parent = from_resolved.parent(&self.shadow);
        let from_child = child_index_at_offset(parent, from_resolved.parent_offset)?;
        let to_child = child_index_at_offset(parent, to_resolved.parent_offset)?;
        if from_child > to_child {
            return None;
        }
        let shadow_path: Vec<u32> = from_resolved.node_path.iter().copied().collect();
        match self.locate(&shadow_path)? {
            LocatedTarget::Created {
                parent,
                entry,
                relative,
            } => self.fold_created(&parent, entry, &relative, &|node| {
                let children = node.content()?.children();
                let start = usize::try_from(from_child).ok()?;
                let end = usize::try_from(to_child).ok()?;
                if end > children.len() {
                    return None;
                }
                let mut rebuilt = children.to_vec();
                rebuilt.splice(start..end, content.iter().cloned());
                (!node.is_void() && !node.is_text()).then(|| {
                    Node::element(
                        node.node_type().to_owned(),
                        node.attrs().clone(),
                        Fragment::from(rebuilt),
                    )
                })
            }),
            LocatedTarget::Base(base_parent) => {
                if self
                    .texts
                    .iter()
                    .any(|text| text.parent_path == base_parent)
                {
                    return None;
                }
                let plan = self.plan_for(&base_parent)?;
                let start = usize::try_from(from_child).ok()?;
                let end = usize::try_from(to_child).ok()?;
                if end > plan.entries.len() {
                    return None;
                }
                let removed = plan
                    .entries
                    .splice(
                        start..end,
                        content.iter().cloned().map(ChildOrigin::Created),
                    )
                    .collect::<Vec<_>>();
                for origin in removed {
                    let ChildOrigin::Base(index) = origin else {
                        continue;
                    };
                    let mut deleted = base_parent.clone();
                    deleted.push(index);
                    self.forget_subtree(&deleted);
                }
                Some(())
            }
        }
    }

    fn fold_created(
        &mut self,
        base_parent: &[u32],
        entry: usize,
        relative: &[u32],
        apply: &dyn Fn(&Node) -> Option<Node>,
    ) -> Option<()> {
        let plan = self.plans.get_mut(base_parent)?;
        let ChildOrigin::Created(node) = plan.entries.get(entry)? else {
            return None;
        };
        let rebuilt = rebuild_at(node, relative, apply)?;
        *plan.entries.get_mut(entry)? = ChildOrigin::Created(rebuilt);
        Some(())
    }

    fn absorb_patch(&mut self, pos: u32, attrs: &HashMap<String, Value>) -> Option<()> {
        let resolved = self.shadow.resolve(pos).ok()?;
        let parent = resolved.parent(&self.shadow);
        let child = child_index_at_offset(parent, resolved.parent_offset)?;
        let mut shadow_path: Vec<u32> = resolved.node_path.iter().copied().collect();
        shadow_path.push(child);
        match self.locate(&shadow_path)? {
            LocatedTarget::Created {
                parent,
                entry,
                relative,
            } => self.fold_created(&parent, entry, &relative, &|node| {
                if node.is_text() {
                    return None;
                }
                if node.is_void() {
                    return Some(Node::void(node.node_type().to_owned(), attrs.clone()));
                }
                Some(Node::element(
                    node.node_type().to_owned(),
                    attrs.clone(),
                    node.content().cloned().unwrap_or_else(Fragment::empty),
                ))
            }),
            LocatedTarget::Base(base_path) => {
                match self
                    .patches
                    .iter_mut()
                    .find(|(existing, _)| *existing == base_path)
                {
                    Some((_, existing)) => *existing = attrs.clone(),
                    None => self.patches.push((base_path, attrs.clone())),
                }
                Some(())
            }
        }
    }

    fn absorb_text(&mut self, pos: u32, text: &str, marks: &[Mark]) -> Option<()> {
        if text.is_empty() {
            return None;
        }
        let resolved = self.shadow.resolve(pos).ok()?;
        let shadow_path: Vec<u32> = resolved.node_path.iter().copied().collect();
        match self.locate(&shadow_path)? {
            LocatedTarget::Created {
                parent,
                entry,
                relative,
            } => {
                let parent_offset = resolved.parent_offset;
                let inserted = Node::text(text.to_owned(), marks.to_vec());
                self.fold_created(&parent, entry, &relative, &|node| {
                    let content = node.content()?;
                    let mut cursor = 0u32;
                    let mut children = content.children().to_vec();
                    for (index, child) in content.iter().enumerate() {
                        if cursor == parent_offset {
                            children.insert(index, inserted.clone());
                            return (!node.is_void() && !node.is_text()).then(|| {
                                Node::element(
                                    node.node_type().to_owned(),
                                    node.attrs().clone(),
                                    Fragment::from(children),
                                )
                            });
                        }
                        cursor = cursor.checked_add(child.node_size())?;
                    }
                    if cursor != parent_offset {
                        return None;
                    }
                    children.push(inserted.clone());
                    (!node.is_void() && !node.is_text()).then(|| {
                        Node::element(
                            node.node_type().to_owned(),
                            node.attrs().clone(),
                            Fragment::from(children),
                        )
                    })
                })
            }
            LocatedTarget::Base(base_parent) => {
                if self.plans.contains_key(&base_parent) {
                    return None;
                }
                self.texts.push(PendingText {
                    parent_path: base_parent,
                    parent_offset: resolved.parent_offset,
                    text: text.to_owned(),
                    marks: marks.to_vec(),
                });
                Some(())
            }
        }
    }

    fn into_edits(self) -> Option<Vec<StructuralEdit>> {
        let mut edits = Vec::new();
        let mut parents = self.plans.into_iter().collect::<Vec<_>>();
        parents.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (base_parent, plan) in parents {
            let base_children = u32::try_from(
                self.base
                    .node_at(&base_parent)?
                    .content()
                    .map_or(NO_CHILDREN, Fragment::child_count),
            )
            .ok()?;
            let mut cursor = FIRST_CHILD;
            let mut window_start: Option<u32> = None;
            let mut created: Vec<Node> = Vec::new();
            for origin in &plan.entries {
                match origin {
                    ChildOrigin::Created(node) => {
                        window_start.get_or_insert(cursor);
                        created.push(node.clone());
                    }
                    ChildOrigin::Base(index) => {
                        if *index < cursor {
                            return None;
                        }
                        if *index > cursor {
                            window_start.get_or_insert(cursor);
                            cursor = *index;
                        }
                        if let Some(start) = window_start.take() {
                            edits.push(StructuralEdit::SpliceChildren {
                                parent_path: base_parent.clone(),
                                from_child: start,
                                to_child: cursor,
                                content: Fragment::from(std::mem::take(&mut created)),
                            });
                        }
                        cursor = index.checked_add(1)?;
                    }
                }
            }
            if cursor < base_children {
                window_start.get_or_insert(cursor);
                cursor = base_children;
            }
            if let Some(start) = window_start {
                edits.push(StructuralEdit::SpliceChildren {
                    parent_path: base_parent.clone(),
                    from_child: start,
                    to_child: cursor,
                    content: Fragment::from(created),
                });
            }
        }
        for (path, attrs) in self.patches {
            if path == ROOT_PATH {
                return None;
            }
            edits.push(StructuralEdit::PatchAttributes { path, attrs });
        }
        for text in self.texts {
            edits.push(StructuralEdit::InsertContentText {
                parent_path: text.parent_path,
                parent_offset: text.parent_offset,
                text: text.text,
                marks: text.marks,
            });
        }
        Some(edits)
    }
}
