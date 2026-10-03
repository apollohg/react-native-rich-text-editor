use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::{attribute_key, attribute_key_bytes, ATTRIBUTE_DIGEST_BYTES, ATTRIBUTE_KEY_BYTES};

#[derive(Clone, Debug)]
pub struct TableContentFingerprint {
    digest: [u8; ATTRIBUTE_DIGEST_BYTES],
    cell: u64,
}

#[derive(Clone, Debug)]
pub enum TableContentKey {
    Digest(Arc<TableContentFingerprint>),
    Text(String),
}

const _: () = {
    assert!(std::mem::size_of::<TableContentKey>() == std::mem::size_of::<String>());
    assert!(std::mem::align_of::<TableContentKey>() == std::mem::align_of::<String>());
    assert!(
        std::mem::size_of::<TableContentFingerprint>() + std::mem::size_of::<[usize; 3]>()
            <= ATTRIBUTE_KEY_BYTES
    );
};

impl From<String> for TableContentKey {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl TableContentKey {
    pub(super) fn new(digest: [u8; ATTRIBUTE_DIGEST_BYTES], cell: u64) -> Self {
        Self::Digest(Arc::new(TableContentFingerprint { digest, cell }))
    }

    pub(crate) fn cell_fingerprint(&self) -> Option<u64> {
        match self {
            Self::Digest(value) => Some(value.cell),
            Self::Text(_) => None,
        }
    }

    pub(crate) fn to_owned_string(&self) -> String {
        match self {
            Self::Digest(value) => attribute_key(&value.digest),
            Self::Text(value) => value.clone(),
        }
    }

    pub(crate) fn retained_string_capacity(&self) -> usize {
        match self {
            Self::Digest(_) => ATTRIBUTE_KEY_BYTES,
            Self::Text(value) => value.capacity(),
        }
    }
}

impl PartialEq for TableContentKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Digest(left), Self::Digest(right)) => left.digest == right.digest,
            (Self::Text(left), Self::Text(right)) => left == right,
            (Self::Digest(digest), Self::Text(text)) | (Self::Text(text), Self::Digest(digest)) => {
                attribute_key_bytes(&digest.digest) == text.as_bytes()
            }
        }
    }
}

impl Eq for TableContentKey {}

impl Hash for TableContentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Digest(value) => {
                let bytes = attribute_key_bytes(&value.digest);
                std::str::from_utf8(&bytes)
                    .expect("hexadecimal is UTF-8")
                    .hash(state);
            }
            Self::Text(value) => value.hash(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::DefaultHasher;

    fn hash(value: &impl Hash) -> u64 {
        let mut hash = DefaultHasher::new();
        value.hash(&mut hash);
        hash.finish()
    }

    #[test]
    fn key_equality_and_hash_ignore_the_cell_fingerprint() {
        let digest = std::array::from_fn(|index| index as u8);
        let left = TableContentKey::new(digest, 7);
        let right = TableContentKey::new(digest, 42);
        let text = attribute_key(&digest);
        let fallback = TableContentKey::from(text.clone());
        assert_eq!(left, right);
        assert_eq!(left, fallback);
        assert_eq!(fallback, right);
        assert_eq!(hash(&left), hash(&text));
        assert_eq!(hash(&right), hash(&text));
        assert_eq!(hash(&fallback), hash(&text));
        assert_eq!(left.cell_fingerprint(), Some(7));
        assert_eq!(right.cell_fingerprint(), Some(42));
        assert_eq!(fallback.cell_fingerprint(), None);
        assert_ne!(left, TableContentKey::from(text.to_uppercase()));
        assert_ne!(left, TableContentKey::new([0; ATTRIBUTE_DIGEST_BYTES], 7));
    }

    #[test]
    fn key_ownership_preserves_legacy_string_charges_and_clone_normalization() {
        let key = TableContentKey::new([0; ATTRIBUTE_DIGEST_BYTES], 7);
        let clone = key.clone();
        let TableContentKey::Digest(value) = &key else {
            unreachable!()
        };
        let weak = Arc::downgrade(value);
        assert_eq!(
            key.retained_string_capacity(),
            key.to_owned_string().capacity()
        );
        assert_eq!(
            clone.retained_string_capacity(),
            key.retained_string_capacity()
        );
        drop(key);
        assert!(weak.upgrade().is_some());
        assert_eq!(clone.cell_fingerprint(), Some(7));
        drop(clone);
        assert!(weak.upgrade().is_none());
        for text in ["", "not a digest 🦀", "0123456789abcdef"] {
            let mut original = String::with_capacity(ATTRIBUTE_KEY_BYTES * 2);
            original.push_str(text);
            let clone_capacity = original.clone().capacity();
            let capacity = original.capacity();
            let key = TableContentKey::from(original);
            assert_eq!(key.to_owned_string(), text);
            assert_eq!(key.cell_fingerprint(), None);
            assert_eq!(key.retained_string_capacity(), capacity);
            assert_eq!(key.clone().retained_string_capacity(), clone_capacity);
        }
    }
}
