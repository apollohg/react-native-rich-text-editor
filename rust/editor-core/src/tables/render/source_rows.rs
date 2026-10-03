use std::ops::Deref;
use std::sync::Arc;

use super::{TableRowAttributeKey, TableSourceRow, ATTRIBUTE_DIGEST_BYTES, ATTRIBUTE_KEY_BYTES};

#[derive(Clone, Debug)]
enum Storage {
    Owned(Vec<TableSourceRow>),
    Shared(Arc<Vec<TableSourceRow>>),
}

#[derive(Clone, Debug)]
pub struct TableSourceRows(Storage);

const _: () = {
    assert!(std::mem::size_of::<TableSourceRows>() == std::mem::size_of::<Vec<TableSourceRow>>());
    assert!(std::mem::align_of::<TableSourceRows>() == std::mem::align_of::<Vec<TableSourceRow>>());
};

impl Default for TableSourceRows {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl From<Vec<TableSourceRow>> for TableSourceRows {
    fn from(rows: Vec<TableSourceRow>) -> Self {
        Self(Storage::Owned(rows))
    }
}

impl Deref for TableSourceRows {
    type Target = [TableSourceRow];

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Owned(rows) => rows,
            Storage::Shared(rows) => rows,
        }
    }
}

impl<'a> IntoIterator for &'a TableSourceRows {
    type Item = &'a TableSourceRow;
    type IntoIter = std::slice::Iter<'a, TableSourceRow>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PartialEq for TableSourceRows {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.deref(), other.deref()) || self.deref() == other.deref()
    }
}

impl TableSourceRows {
    pub(super) fn into_shared(self) -> Self {
        match self.0 {
            Storage::Owned(rows) if sharing_is_funded(&rows) => {
                Self(Storage::Shared(Arc::new(rows)))
            }
            storage => Self(storage),
        }
    }
}

fn sharing_is_funded(rows: &Vec<TableSourceRow>) -> bool {
    let savings = crate::model::arc_allocation_retained_bytes(ATTRIBUTE_DIGEST_BYTES)
        .and_then(|bytes| ATTRIBUTE_KEY_BYTES.checked_sub(bytes))
        .and_then(|bytes| bytes.checked_mul(rows.len()));
    let overhead =
        crate::model::arc_allocation_retained_bytes(std::mem::size_of::<Vec<TableSourceRow>>());
    // Digest savings fund ownership; exact capacity preserves the prior clone's row buffer.
    rows.capacity() == rows.len()
        && savings
            .zip(overhead)
            .is_some_and(|(saved, overhead)| saved >= overhead)
        && rows
            .iter()
            .all(|row| matches!(row.attrs_key, TableRowAttributeKey::Digest(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_rows(count: usize, spare: usize) -> Vec<TableSourceRow> {
        let mut rows = Vec::with_capacity(count + spare);
        for index in 0..count {
            rows.push(TableSourceRow {
                attrs_key: TableRowAttributeKey::Digest(Arc::new(
                    [index as u8; ATTRIBUTE_DIGEST_BYTES],
                )),
                cell_count: index as u32,
            });
        }
        rows
    }

    fn minimum_funded_rows() -> usize {
        let saved = ATTRIBUTE_KEY_BYTES
            - crate::model::arc_allocation_retained_bytes(ATTRIBUTE_DIGEST_BYTES).unwrap();
        crate::model::arc_allocation_retained_bytes(std::mem::size_of::<Vec<TableSourceRow>>())
            .unwrap()
            .div_ceil(saved)
    }

    fn legacy_charge(rows: &[TableSourceRow]) -> usize {
        rows.iter()
            .map(|row| {
                std::mem::size_of::<TableSourceRow>() + row.attrs_key.retained_string_capacity()
            })
            .sum()
    }

    #[test]
    fn sharing_requires_funded_metadata_and_exact_digest_storage() {
        let minimum = minimum_funded_rows();
        for count in [0, 1, minimum - 1, minimum, minimum + 1] {
            for spare in [0, 1] {
                let original = digest_rows(count, spare);
                let old_charge = legacy_charge(&original);
                let owned = TableSourceRows::from(original);
                let expected = owned.clone();
                let candidate = owned.into_shared();
                assert_eq!(candidate, expected, "count={count} spare={spare}");
                assert_eq!(legacy_charge(&candidate), old_charge);
                assert_eq!(legacy_charge(&candidate.clone()), legacy_charge(&expected));
                let shared = matches!(candidate.0, Storage::Shared(_));
                assert_eq!(
                    shared,
                    count >= minimum && spare == 0,
                    "count={count} spare={spare}"
                );
                if shared {
                    let physical = std::mem::size_of::<TableSourceRow>() * count
                        + crate::model::arc_allocation_retained_bytes(ATTRIBUTE_DIGEST_BYTES)
                            .unwrap()
                            * count
                        + crate::model::arc_allocation_retained_bytes(std::mem::size_of::<
                            Vec<TableSourceRow>,
                        >())
                        .unwrap();
                    assert!(
                        physical <= old_charge,
                        "physical={physical} legacy={old_charge}"
                    );
                    assert_eq!(candidate.as_ptr(), candidate.clone().as_ptr());
                }
            }
        }
    }

    #[test]
    fn text_fallbacks_keep_their_original_clone_capacity_normalization() {
        const SPARE_BYTES: usize = 19;
        const TEXT_BYTES: usize = 7;
        for text_len in [TEXT_BYTES, ATTRIBUTE_KEY_BYTES] {
            for spare in [0, SPARE_BYTES] {
                let mut rows = digest_rows(minimum_funded_rows() + 1, 0);
                let mut text = String::with_capacity(text_len + spare);
                text.extend(std::iter::repeat_n('a', text_len));
                rows[0].attrs_key = TableRowAttributeKey::Text(text);
                let expected = rows.clone();
                let original_charge = legacy_charge(&rows);
                let candidate = TableSourceRows::from(rows).into_shared();
                assert!(matches!(candidate.0, Storage::Owned(_)));
                assert_eq!(legacy_charge(&candidate), original_charge);
                assert_eq!(&*candidate.clone(), expected.as_slice());
                assert_eq!(legacy_charge(&candidate.clone()), legacy_charge(&expected));
                assert_eq!(
                    candidate.clone()[0].attrs_key.retained_string_capacity(),
                    text_len
                );
            }
        }
    }

    #[test]
    fn shared_rows_release_the_array_and_keys_after_the_last_owner() {
        let original = TableSourceRows::from(digest_rows(minimum_funded_rows(), 0)).into_shared();
        let Storage::Shared(rows) = &original.0 else {
            panic!("funded rows were not shared");
        };
        let weak_rows = Arc::downgrade(rows);
        let TableRowAttributeKey::Digest(key) = &rows[0].attrs_key else {
            unreachable!()
        };
        let weak_key = Arc::downgrade(key);
        let cloned = original.clone();
        assert_eq!(original.as_ptr(), cloned.as_ptr());
        assert_eq!(
            weak_key.strong_count(),
            1,
            "cloning a row array must not clone each key"
        );
        drop(original);
        assert!(weak_rows.upgrade().is_some());
        assert_eq!(cloned[0].cell_count, 0);
        drop(cloned);
        assert!(weak_rows.upgrade().is_none());
        assert!(weak_key.upgrade().is_none());
    }
}
