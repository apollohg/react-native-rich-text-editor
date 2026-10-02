use std::sync::Arc;

use super::{attribute_key, ATTRIBUTE_DIGEST_BYTES, ATTRIBUTE_KEY_BYTES};

#[derive(Clone, Debug)]
pub enum TableRowAttributeKey {
    Digest(Arc<[u8; ATTRIBUTE_DIGEST_BYTES]>),
    Text(String),
}

const _: () = {
    assert!(std::mem::size_of::<TableRowAttributeKey>() == std::mem::size_of::<String>());
    assert!(std::mem::align_of::<TableRowAttributeKey>() == std::mem::align_of::<String>());
    assert!(ATTRIBUTE_DIGEST_BYTES + std::mem::size_of::<[usize; 3]>() <= ATTRIBUTE_KEY_BYTES);
};

impl From<String> for TableRowAttributeKey {
    fn from(value: String) -> Self {
        if value.capacity() == ATTRIBUTE_KEY_BYTES {
            if let Some(digest) = parse_digest(&value) {
                drop(value);
                return Self::Digest(Arc::new(digest));
            }
        }
        Self::Text(value)
    }
}

impl PartialEq for TableRowAttributeKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Digest(left), Self::Digest(right)) => left == right,
            (Self::Text(left), Self::Text(right)) => left == right,
            (Self::Digest(digest), Self::Text(text)) | (Self::Text(text), Self::Digest(digest)) => {
                parse_digest(text).as_ref() == Some(digest.as_ref())
            }
        }
    }
}

impl TableRowAttributeKey {
    pub(crate) fn to_owned_string(&self) -> String {
        match self {
            Self::Digest(digest) => attribute_key(digest),
            Self::Text(text) => text.clone(),
        }
    }

    pub(crate) fn retained_string_capacity(&self) -> usize {
        match self {
            // Keep the prior String charge and its clone normalization.
            Self::Digest(_) => ATTRIBUTE_KEY_BYTES,
            Self::Text(text) => text.capacity(),
        }
    }

    #[cfg(test)]
    pub(super) fn as_ptr(&self) -> *const u8 {
        match self {
            Self::Digest(digest) => digest.as_ptr(),
            Self::Text(text) => text.as_ptr(),
        }
    }
}

fn parse_digest(text: &str) -> Option<[u8; ATTRIBUTE_DIGEST_BYTES]> {
    if text.len() != ATTRIBUTE_KEY_BYTES {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut digest = [0; ATTRIBUTE_DIGEST_BYTES];
    for (output, pair) in digest.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *output = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_keys_preserve_exact_strings_and_clone_charges() {
        const SPARE_BYTES: usize = 19;
        for byte in u8::MIN..=u8::MAX {
            let digest = [byte; ATTRIBUTE_DIGEST_BYTES];
            let expected: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(
                attribute_key(&digest).capacity(),
                expected.capacity(),
                "byte={byte}"
            );
            let key = TableRowAttributeKey::from(expected.clone());
            assert!(
                matches!(key, TableRowAttributeKey::Digest(_)),
                "byte={byte}"
            );
            assert_eq!(key.to_owned_string(), expected, "byte={byte}");
            assert_eq!(
                key.to_owned_string().capacity(),
                expected.clone().capacity()
            );
            assert_eq!(key.retained_string_capacity(), expected.clone().capacity());
            let mut spare = String::with_capacity(ATTRIBUTE_KEY_BYTES + SPARE_BYTES);
            spare.push_str(&expected);
            let previous_capacity = spare.capacity();
            let fallback = TableRowAttributeKey::from(spare);
            assert!(matches!(fallback, TableRowAttributeKey::Text(_)));
            assert_eq!(fallback.retained_string_capacity(), previous_capacity);
            assert_eq!(
                fallback.clone().retained_string_capacity(),
                expected.clone().capacity()
            );
            assert_eq!(key, fallback);
            assert_eq!(fallback, key);
        }
        for text in [
            String::new(),
            "x".to_owned(),
            "a".repeat(ATTRIBUTE_KEY_BYTES - 1),
            "a".repeat(ATTRIBUTE_KEY_BYTES + 1),
            "F".repeat(ATTRIBUTE_KEY_BYTES),
            "é".repeat(ATTRIBUTE_DIGEST_BYTES),
            "\0".repeat(ATTRIBUTE_KEY_BYTES),
        ] {
            for spare in [0, SPARE_BYTES] {
                let mut input = String::with_capacity(text.len() + spare);
                input.push_str(&text);
                let previous_capacity = input.capacity();
                let expected_clone = input.clone();
                let key = TableRowAttributeKey::from(input);
                assert!(
                    matches!(key, TableRowAttributeKey::Text(_)),
                    "text={text:?} spare={spare}"
                );
                assert_eq!(key.to_owned_string(), text);
                assert_eq!(key.to_owned_string().capacity(), expected_clone.capacity());
                assert_eq!(key.retained_string_capacity(), previous_capacity);
                assert_eq!(
                    key.clone().retained_string_capacity(),
                    expected_clone.capacity()
                );
            }
        }
        assert_ne!(
            TableRowAttributeKey::from("0".repeat(ATTRIBUTE_KEY_BYTES)),
            TableRowAttributeKey::from("1".repeat(ATTRIBUTE_KEY_BYTES))
        );
    }

    #[test]
    fn shared_row_key_allocation_fits_legacy_charge_and_releases_with_last_owner() {
        let expected = "a".repeat(ATTRIBUTE_KEY_BYTES);
        let key = TableRowAttributeKey::from(expected.clone());
        let TableRowAttributeKey::Digest(digest) = &key else {
            panic!("canonical key must share")
        };
        let weak = Arc::downgrade(digest);
        let allocated =
            crate::model::arc_allocation_retained_bytes(ATTRIBUTE_DIGEST_BYTES).unwrap();
        assert!(allocated <= key.retained_string_capacity());
        let clone = key.clone();
        assert_eq!(key.as_ptr(), clone.as_ptr());
        assert_eq!(weak.strong_count(), 2);
        drop(key);
        assert_eq!(clone.to_owned_string(), expected);
        assert_eq!(weak.strong_count(), 1);
        drop(clone);
        assert!(weak.upgrade().is_none());
    }
}
