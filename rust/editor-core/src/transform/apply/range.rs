const NODE_OPENING_TOKENS: u32 = 1;
const NODE_CLOSING_TOKENS: u32 = 1;

/// Insert a node into a parent's children at the given parent-content offset.
///
/// Works for both block-level insertion (between element children) and
/// inline-level insertion (between text/void children within a text block).
fn insert_node_in_children(parent: &Node, offset: u32, insert_node: &Node) -> Vec<Node> {
    let content = parent.content().expect("parent should be an element node");
    let mut new_children: Vec<Node> = Vec::with_capacity(content.child_count() + 2);
    let mut remaining_offset = offset;

    if content.child_count() == 0 {
        new_children.push(insert_node.clone());
        return new_children;
    }

    let mut inserted = false;

    for child in content.iter() {
        if inserted {
            new_children.push(child.clone());
            continue;
        }

        let child_size = child.node_size();

        if child.is_text() {
            if remaining_offset <= child_size {
                if remaining_offset == 0 {
                    new_children.push(insert_node.clone());
                    new_children.push(child.clone());
                    inserted = true;
                    continue;
                } else if remaining_offset == child_size {
                    // Insert after this text node — continue to next child or
                    // insert after all children.
                    new_children.push(child.clone());
                    remaining_offset -= child_size;
                    continue;
                } else {
                    let (left, right) = split_text_node(child, remaining_offset);
                    if let Some(l) = left {
                        new_children.push(l);
                    }
                    new_children.push(insert_node.clone());
                    if let Some(r) = right {
                        new_children.push(r);
                    }
                    inserted = true;
                    continue;
                }
            }
            new_children.push(child.clone());
            remaining_offset -= child_size;
        } else if child.is_void() {
            if remaining_offset == 0 {
                new_children.push(insert_node.clone());
                new_children.push(child.clone());
                inserted = true;
                continue;
            }
            remaining_offset -= 1;
            new_children.push(child.clone());
        } else {
            // Element child — offset must be at a boundary (before or after this child).
            if remaining_offset == 0 {
                new_children.push(insert_node.clone());
                new_children.push(child.clone());
                inserted = true;
                continue;
            }
            remaining_offset -= child_size;
            new_children.push(child.clone());
        }
    }

    if !inserted {
        new_children.push(insert_node.clone());
    }

    merge_adjacent_text_nodes(new_children)
}

fn apply_replace_range(
    doc: std::borrow::Cow<'_, Document>,
    from: u32,
    to: u32,
    content: &Fragment,
    schema: &Schema,
) -> Result<(Document, StepMap), TransformError> {
    if from > to {
        return Err(TransformError::InvalidRange(format!(
            "replace range from ({from}) is greater than to ({to})"
        )));
    }

    let resolved_from = doc.resolve(from).map_err(TransformError::OutOfBounds)?;

    if from == to && content.size() == 0 {
        return Ok((doc.into_owned(), StepMap::empty()));
    }

    if from != to {
        let resolved_to = doc.resolve(to).map_err(TransformError::OutOfBounds)?;

        if resolved_from.node_path != resolved_to.node_path {
            return apply_cross_parent_replace(
                &doc,
                from,
                to,
                content,
                &resolved_from,
                &resolved_to,
                schema,
            );
        }
    }

    let parent = resolved_from.parent(&doc);
    let from_offset = resolved_from.parent_offset;
    let deleted_len = to - from;
    let to_offset = from_offset + deleted_len;

    let after_delete = if deleted_len > 0 {
        delete_in_children(parent, from_offset, to_offset)
    } else {
        parent
            .content()
            .expect("parent should be an element node")
            .iter()
            .cloned()
            .collect()
    };

    let after_insert = if content.size() > 0 {
        // Build a temporary parent with the after-delete children so we can
        // use insert_nodes_in_children to splice in the content.
        let temp_parent = rebuild_element(parent, after_delete);
        insert_nodes_in_children(&temp_parent, from_offset, content)
    } else {
        after_delete
    };

    let new_parent = rebuild_element(parent, after_insert);
    let new_doc = match doc {
        std::borrow::Cow::Borrowed(doc) => Document::new(replace_node_at_path(
            doc.root(),
            &resolved_from.node_path,
            &new_parent,
        )),
        std::borrow::Cow::Owned(doc) => {
            doc.replace_node_at_path(&resolved_from.node_path, new_parent)
        }
    };

    let inserted_size = content.size();
    let map = StepMap::from_replace(from, deleted_len, inserted_size);

    Ok((new_doc, map))
}

/// Insert multiple nodes (from a Fragment) into a parent's children at the
/// given parent-content offset.
fn insert_nodes_in_children(parent: &Node, offset: u32, fragment: &Fragment) -> Vec<Node> {
    let content = parent.content().expect("parent should be an element node");
    let insert_nodes: Vec<&Node> = fragment.iter().collect();
    let mut new_children: Vec<Node> =
        Vec::with_capacity(content.child_count() + insert_nodes.len() + 2);
    let mut remaining_offset = offset;

    if content.child_count() == 0 {
        for node in &insert_nodes {
            new_children.push((*node).clone());
        }
        return merge_adjacent_text_nodes(new_children);
    }

    let mut inserted = false;

    for child in content.iter() {
        if inserted {
            new_children.push(child.clone());
            continue;
        }

        let child_size = child.node_size();

        if child.is_text() {
            if remaining_offset <= child_size {
                if remaining_offset == 0 {
                    for node in &insert_nodes {
                        new_children.push((*node).clone());
                    }
                    new_children.push(child.clone());
                    inserted = true;
                    continue;
                } else if remaining_offset == child_size {
                    new_children.push(child.clone());
                    remaining_offset -= child_size;
                    continue;
                } else {
                    let (left, right) = split_text_node(child, remaining_offset);
                    if let Some(l) = left {
                        new_children.push(l);
                    }
                    for node in &insert_nodes {
                        new_children.push((*node).clone());
                    }
                    if let Some(r) = right {
                        new_children.push(r);
                    }
                    inserted = true;
                    continue;
                }
            }
            new_children.push(child.clone());
            remaining_offset -= child_size;
        } else if child.is_void() {
            if remaining_offset == 0 {
                for node in &insert_nodes {
                    new_children.push((*node).clone());
                }
                new_children.push(child.clone());
                inserted = true;
                continue;
            }
            remaining_offset -= 1;
            new_children.push(child.clone());
        } else {
            if remaining_offset == 0 {
                for node in &insert_nodes {
                    new_children.push((*node).clone());
                }
                new_children.push(child.clone());
                inserted = true;
                continue;
            }
            remaining_offset -= child_size;
            new_children.push(child.clone());
        }
    }

    if !inserted {
        for node in &insert_nodes {
            new_children.push((*node).clone());
        }
    }

    merge_adjacent_text_nodes(new_children)
}

use crate::model::ResolvedPos;

/// Delete content that spans two different parent blocks. The common case is
/// two sibling blocks under the same grandparent (e.g. selecting across two
/// paragraphs). This function:
/// 1. Keeps content before `from` in the first block
/// 2. Removes all blocks between the first and last
/// 3. Keeps content after `to` in the last block
/// 4. Merges the remaining content of first and last blocks into one block
fn apply_cross_parent_delete(
    doc: &Document,
    from: u32,
    to: u32,
    resolved_from: &ResolvedPos,
    resolved_to: &ResolvedPos,
    schema: &Schema,
) -> Result<(Document, StepMap), TransformError> {
    let common_depth = common_prefix_len(&resolved_from.node_path, &resolved_to.node_path);
    let common_path = &resolved_from.node_path[..common_depth];
    let common_ancestor = doc
        .node_at(common_path)
        .ok_or_else(|| TransformError::OutOfBounds("common ancestor path invalid".to_string()))?;
    let common_content = common_ancestor.content().ok_or_else(|| {
        TransformError::InvalidTarget("common ancestor has no content".to_string())
    })?;
    let deleted_len = to - from;
    let rebuilt = |children: Vec<Node>| {
        Document::new(replace_node_at_path(
            doc.root(),
            common_path,
            &rebuild_element(common_ancestor, children),
        ))
    };
    if resolved_to.node_path.len() == common_depth {
        let first_child_idx = *resolved_from.node_path.get(common_depth).ok_or_else(|| {
            TransformError::InvalidRange("cross-parent delete: empty range".to_string())
        })? as usize;
        let first_inner_path = &resolved_from.node_path[common_depth + 1..];
        let first_block = common_content
            .child(first_child_idx)
            .ok_or_else(|| TransformError::OutOfBounds("first block not found".to_string()))?;
        let mut children: Vec<Node> = common_content
            .iter()
            .take(first_child_idx)
            .cloned()
            .collect();
        children.push(truncate_after(
            first_block,
            first_inner_path,
            resolved_from.parent_offset,
        ));
        children.extend(
            common_content
                .iter()
                .skip(child_index_at_offset(
                    common_ancestor,
                    resolved_to.parent_offset,
                ))
                .cloned(),
        );
        return Ok((
            rebuilt(children),
            StepMap::from_replace(
                from,
                deleted_len,
                NODE_CLOSING_TOKENS * (first_inner_path.len() as u32 + 1),
            ),
        ));
    }
    if resolved_from.node_path.len() == common_depth {
        let last_child_idx = resolved_to.node_path[common_depth] as usize;
        let last_inner_path = &resolved_to.node_path[common_depth + 1..];
        let last_block = common_content
            .child(last_child_idx)
            .ok_or_else(|| TransformError::OutOfBounds("last block not found".to_string()))?;
        let mut children: Vec<Node> = common_content
            .iter()
            .take(child_index_at_offset(
                common_ancestor,
                resolved_from.parent_offset,
            ))
            .cloned()
            .collect();
        children.push(truncate_before(
            last_block,
            last_inner_path,
            resolved_to.parent_offset,
        ));
        children.extend(common_content.iter().skip(last_child_idx + 1).cloned());
        return Ok((
            rebuilt(children),
            StepMap::from_replace(
                from,
                deleted_len,
                NODE_OPENING_TOKENS * (last_inner_path.len() as u32 + 1),
            ),
        ));
    }
    let first_child_idx = *resolved_from.node_path.get(common_depth).ok_or_else(|| {
        TransformError::InvalidRange(
            "cross-parent delete: from endpoint resolves to common ancestor boundary".to_string(),
        )
    })? as usize;
    let last_child_idx = *resolved_to.node_path.get(common_depth).ok_or_else(|| {
        TransformError::InvalidRange(
            "cross-parent delete: to endpoint resolves to common ancestor boundary".to_string(),
        )
    })? as usize;
    if first_child_idx >= last_child_idx {
        return Err(TransformError::InvalidRange(
            "cross-parent delete: first block index >= last block index".to_string(),
        ));
    }
    let first_block = common_content
        .child(first_child_idx)
        .ok_or_else(|| TransformError::OutOfBounds("first block not found".to_string()))?;
    let last_block = common_content
        .child(last_child_idx)
        .ok_or_else(|| TransformError::OutOfBounds("last block not found".to_string()))?;
    let first_inner_path = &resolved_from.node_path[common_depth + 1..];
    let last_inner_path = &resolved_to.node_path[common_depth + 1..];
    let last_parent = resolved_to.parent(doc);
    if !leaf_blocks_join(doc, resolved_from, resolved_to, schema) {
        let left = truncate_after(first_block, first_inner_path, resolved_from.parent_offset);
        let right = truncate_before(last_block, last_inner_path, resolved_to.parent_offset);
        let left_depth = first_inner_path.len() + 1;
        let right_depth = last_inner_path.len() + 1;
        let (merge_depth, joined) = (1..=left_depth.min(right_depth))
            .rev()
            .find_map(|depth| {
                merge_at_depth(&left, &right, depth - 1, schema).map(|merged| (depth, vec![merged]))
            })
            .unwrap_or((0, vec![left, right]));
        let mut new_common_children: Vec<Node> = Vec::with_capacity(common_content.child_count());
        new_common_children.extend(common_content.iter().take(first_child_idx).cloned());
        new_common_children.extend(joined);
        new_common_children.extend(common_content.iter().skip(last_child_idx + 1).cloned());
        let kept_boundaries = NODE_CLOSING_TOKENS * (left_depth - merge_depth) as u32
            + NODE_OPENING_TOKENS * (right_depth - merge_depth) as u32;
        return Ok((
            rebuilt(new_common_children),
            StepMap::from_replace(from, deleted_len, kept_boundaries),
        ));
    }

    let right = truncate_before(last_block, last_inner_path, resolved_to.parent_offset);
    let detached = detach_leading_leaf(right, last_inner_path.len(), schema);
    let left = truncate_after(first_block, first_inner_path, resolved_from.parent_offset);
    let merged_left = append_to_trailing_leaf(left, first_inner_path.len(), detached.leaf_children);

    let mut new_common_children: Vec<Node> = Vec::with_capacity(common_content.child_count());
    new_common_children.extend(common_content.iter().take(first_child_idx).cloned());
    new_common_children.push(merged_left);
    new_common_children.extend(detached.remainder);
    new_common_children.extend(common_content.iter().skip(last_child_idx + 1).cloned());
    let mut map = StepMap::from_delete(from, deleted_len);
    let removed_closings = NODE_CLOSING_TOKENS * (1 + detached.pruned_ancestors);
    let restored_boundaries = NODE_CLOSING_TOKENS * (first_inner_path.len() as u32 + 1)
        + NODE_OPENING_TOKENS * detached.surviving_ancestors
        + (NODE_OPENING_TOKENS + NODE_CLOSING_TOKENS) * detached.filled_text_blocks;
    if removed_closings != restored_boundaries {
        let leaf_end = to + (last_parent.content_size() - resolved_to.parent_offset) - deleted_len;
        let unchanged = removed_closings.min(restored_boundaries);
        map = map.compose(&StepMap::from_replace(
            leaf_end + unchanged,
            removed_closings - unchanged,
            restored_boundaries - unchanged,
        ));
    }
    Ok((rebuilt(new_common_children), map))
}

fn child_index_at_offset(parent: &Node, offset: u32) -> usize {
    let mut consumed = 0u32;
    parent
        .content()
        .map(|content| {
            content
                .iter()
                .take_while(|child| {
                    let fits = consumed + child.node_size() <= offset;
                    if fits {
                        consumed += child.node_size();
                    }
                    fits
                })
                .count()
        })
        .unwrap_or(0)
}

fn merge_at_depth(left: &Node, right: &Node, depth: usize, schema: &Schema) -> Option<Node> {
    let mut children = element_children(left);
    let mut right_children = element_children(right);
    if depth > 0 {
        let left_child = children.pop()?;
        if right_children.is_empty() {
            return None;
        }
        let right_child = right_children.remove(0);
        children.push(merge_at_depth(
            &left_child,
            &right_child,
            depth - 1,
            schema,
        )?);
    }
    children.extend(right_children);
    let children = merge_adjacent_text_nodes(children);
    schema
        .node(left.node_type())?
        .content
        .matches(&children, |child: &Node, symbol| {
            schema.node_matches_symbol(child.node_type(), symbol)
        })
        .then(|| rebuild_element(left, children))
}

pub(crate) fn delete_range_joins_leaf_blocks(
    doc: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
) -> bool {
    let (Ok(resolved_from), Ok(resolved_to)) = (doc.resolve(from), doc.resolve(to)) else {
        return false;
    };
    resolved_from.node_path == resolved_to.node_path
        || leaf_blocks_join(doc, &resolved_from, &resolved_to, schema)
}

fn leaf_blocks_join(
    doc: &Document,
    resolved_from: &ResolvedPos,
    resolved_to: &ResolvedPos,
    schema: &Schema,
) -> bool {
    let first_parent = resolved_from.parent(doc);
    let joined_children = split_children_at(first_parent, resolved_from.parent_offset)
        .0
        .into_iter()
        .chain(split_children_at(resolved_to.parent(doc), resolved_to.parent_offset).1)
        .collect::<Vec<_>>();
    schema.node(first_parent.node_type()).is_some_and(|spec| {
        spec.content
            .matches(&joined_children, |child: &Node, symbol| {
                schema.node_matches_symbol(child.node_type(), symbol)
            })
    })
}

struct DetachedLeaf {
    leaf_children: Vec<Node>,
    remainder: Option<Node>,
    surviving_ancestors: u32,
    pruned_ancestors: u32,
    filled_text_blocks: u32,
}

fn element_children(node: &Node) -> Vec<Node> {
    node.content()
        .map(|content| content.iter().cloned().collect())
        .unwrap_or_default()
}

fn truncate_after(node: &Node, path: &[u32], offset: u32) -> Node {
    let Some((&index, rest)) = path.split_first() else {
        return rebuild_element(node, split_children_at(node, offset).0);
    };
    let mut children = element_children(node);
    children.truncate(index as usize + 1);
    if let Some(last) = children.pop() {
        children.push(truncate_after(&last, rest, offset));
    }
    rebuild_element(node, children)
}

fn truncate_before(node: &Node, path: &[u32], offset: u32) -> Node {
    let Some((&index, rest)) = path.split_first() else {
        return rebuild_element(node, split_children_at(node, offset).1);
    };
    let mut children = element_children(node).split_off(index as usize);
    if !children.is_empty() {
        let first = children.remove(0);
        children.insert(0, truncate_before(&first, rest, offset));
    }
    rebuild_element(node, children)
}

fn detach_leading_leaf(node: Node, depth: usize, schema: &Schema) -> DetachedLeaf {
    let mut children = element_children(&node);
    if depth == 0 || children.is_empty() {
        return DetachedLeaf {
            leaf_children: children,
            remainder: None,
            surviving_ancestors: 0,
            pruned_ancestors: 0,
            filled_text_blocks: 0,
        };
    }
    let first = children.remove(0);
    let mut detached = detach_leading_leaf(first, depth - 1, schema);
    if let Some(remainder) = detached.remainder.take() {
        children.insert(0, remainder);
    }
    if children.is_empty() {
        detached.pruned_ancestors += 1;
    } else {
        detached.surviving_ancestors += 1;
        if let Some(filler) = required_leading_text_block(&node, &children, schema) {
            detached.filled_text_blocks += 1;
            children.insert(0, filler);
        }
        detached.remainder = Some(rebuild_element(&node, children));
    }
    detached
}

pub(crate) fn with_required_leading_text_blocks(node: &Node, schema: &Schema) -> Node {
    let Some(content) = node.content() else {
        return node.clone();
    };
    let mut children = content
        .iter()
        .map(|child| with_required_leading_text_blocks(child, schema))
        .collect::<Vec<_>>();
    if let Some(filler) = required_leading_text_block(node, &children, schema) {
        children.insert(0, filler);
    }
    rebuild_element(node, children)
}

fn required_leading_text_block(parent: &Node, children: &[Node], schema: &Schema) -> Option<Node> {
    let rule = &schema.node(parent.node_type())?.content;
    let accepts = |children: &[Node]| {
        rule.matches(children, |child: &Node, symbol| {
            schema.node_matches_symbol(child.node_type(), symbol)
        })
    };
    if accepts(children) {
        return None;
    }
    let text_block = schema.preferred_text_block()?;
    let filler = Node::element(
        text_block.name.clone(),
        text_block
            .attrs
            .iter()
            .filter_map(|(name, attr)| attr.default.clone().map(|value| (name.clone(), value)))
            .collect(),
        Fragment::empty(),
    );
    let filled = std::iter::once(filler.clone())
        .chain(children.iter().cloned())
        .collect::<Vec<_>>();
    accepts(&filled).then_some(filler)
}

fn append_to_trailing_leaf(node: Node, depth: usize, extra: Vec<Node>) -> Node {
    let mut children = element_children(&node);
    if depth == 0 {
        children.extend(extra);
        return rebuild_element(&node, merge_adjacent_text_nodes(children));
    }
    if let Some(last) = children.pop() {
        children.push(append_to_trailing_leaf(last, depth - 1, extra));
    }
    rebuild_element(&node, children)
}

/// Cross-parent replacement: delete across parent boundaries, then insert content.
fn apply_cross_parent_replace(
    doc: &Document,
    from: u32,
    to: u32,
    content: &Fragment,
    resolved_from: &ResolvedPos,
    resolved_to: &ResolvedPos,
    schema: &Schema,
) -> Result<(Document, StepMap), TransformError> {
    let (after_delete, delete_map) =
        apply_cross_parent_delete(doc, from, to, resolved_from, resolved_to, schema)?;

    if content.size() == 0 {
        return Ok((after_delete, delete_map));
    }

    let resolved_insert = after_delete
        .resolve(from)
        .map_err(TransformError::OutOfBounds)?;

    let parent = resolved_insert.parent(&after_delete);
    let insert_offset = resolved_insert.parent_offset;

    let temp_parent = rebuild_element(parent, parent.content().unwrap().iter().cloned().collect());
    let after_insert = insert_nodes_in_children(&temp_parent, insert_offset, content);
    let new_parent = rebuild_element(parent, after_insert);

    let new_root =
        replace_node_at_path(after_delete.root(), &resolved_insert.node_path, &new_parent);
    let new_doc = Document::new(new_root);

    Ok((
        new_doc,
        delete_map.compose(&StepMap::from_insert(from, content.size())),
    ))
}

/// Find the length of the common prefix of two paths.
fn common_prefix_len(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Replace a node at the given path in the tree with two new nodes, returning
/// a new root. The node at `path` is removed and replaced by `first` and
/// `second` in that order.
fn replace_node_with_two(root: &Node, path: &[u32], first: &Node, second: &Node) -> Node {
    if path.is_empty() {
        panic!("replace_node_with_two called with empty path — cannot replace root with two nodes");
    }

    if path.len() == 1 {
        let content = root
            .content()
            .expect("non-leaf node in path must be an element");
        let replace_idx = path[0] as usize;
        let mut new_children: Vec<Node> = Vec::with_capacity(content.child_count() + 1);

        for (i, child) in content.iter().enumerate() {
            if i == replace_idx {
                new_children.push(first.clone());
                new_children.push(second.clone());
            } else {
                new_children.push(child.clone());
            }
        }

        return rebuild_element(root, new_children);
    }

    let content = root
        .content()
        .expect("non-leaf node in path must be an element");
    let mut new_children: Vec<Node> = Vec::with_capacity(content.child_count() + 1);

    for (i, child) in content.iter().enumerate() {
        if i == path[0] as usize {
            new_children.push(replace_node_with_two(child, &path[1..], first, second));
        } else {
            new_children.push(child.clone());
        }
    }

    rebuild_element(root, new_children)
}

/// Replace a node at the given path with multiple nodes, returning a new root.
///
/// The node at `path` is removed and replaced by all nodes in `replacements`.
fn replace_node_with_many(root: &Node, path: &[u32], replacements: &[Node]) -> Node {
    if path.is_empty() {
        panic!("replace_node_with_many called with empty path — cannot replace root with multiple nodes");
    }

    if path.len() == 1 {
        let content = root
            .content()
            .expect("non-leaf node in path must be an element");
        let replace_idx = path[0] as usize;
        let mut new_children: Vec<Node> =
            Vec::with_capacity(content.child_count() + replacements.len() - 1);

        for (i, child) in content.iter().enumerate() {
            if i == replace_idx {
                new_children.extend(replacements.iter().cloned());
            } else {
                new_children.push(child.clone());
            }
        }

        return rebuild_element(root, new_children);
    }

    let content = root
        .content()
        .expect("non-leaf node in path must be an element");
    let mut new_children: Vec<Node> = Vec::with_capacity(content.child_count());

    for (i, child) in content.iter().enumerate() {
        if i == path[0] as usize {
            new_children.push(replace_node_with_many(child, &path[1..], replacements));
        } else {
            new_children.push(child.clone());
        }
    }

    rebuild_element(root, new_children)
}

/// Replace a node at the given path in the tree, returning a new root.
///
/// `path` is a sequence of child indices from the root. An empty path means
/// replace the root itself.
fn replace_node_at_path(root: &Node, path: &[u32], replacement: &Node) -> Node {
    if path.is_empty() {
        return replacement.clone();
    }

    let content = root
        .content()
        .expect("non-leaf node in path must be an element");
    let mut new_children: Vec<Node> = Vec::with_capacity(content.child_count());

    for (i, child) in content.iter().enumerate() {
        if i == path[0] as usize {
            new_children.push(replace_node_at_path(child, &path[1..], replacement));
        } else {
            new_children.push(child.clone());
        }
    }

    rebuild_element(root, new_children)
}
