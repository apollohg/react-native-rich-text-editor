use super::hash_prefix::Sha256Prefix;
use std::io::Write;

const NUMBER_TOKEN_BYTES: usize = 32;
const NEGATIVE_ZERO_JSON: &[u8] = b"-0.0";
const POSITIVE_ZERO_JSON: &[u8] = b"0.0";

#[cfg(test)]
std::thread_local! {
    static NORMALIZED_INPUT_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn take_normalized_input_bytes_for_test() -> usize {
    NORMALIZED_INPUT_BYTES.replace(0)
}

#[derive(Clone, Debug)]
pub(super) struct ProjectionHashPrefix {
    hash: Sha256Prefix,
    normalizer: ProjectionNormalizer,
}

#[derive(Clone, Debug, Default)]
struct ProjectionNormalizer {
    number: [u8; NUMBER_TOKEN_BYTES],
    number_len: usize,
    in_string: bool,
    escaped: bool,
    changed_bytes: bool,
}

#[derive(Debug)]
pub(super) struct ProjectionHashSuffix {
    normalized: Option<Vec<u8>>,
}

impl ProjectionHashPrefix {
    pub(super) fn new(bytes: &[u8]) -> Option<Self> {
        let mut prefix = Self {
            hash: Sha256Prefix::new(&[])?,
            normalizer: ProjectionNormalizer::default(),
        };
        prefix.write_all(bytes).ok()?;
        Some(prefix)
    }

    pub(super) fn finish(&self, first: &[u8], second: &[u8]) -> Option<[u8; 32]> {
        let mut combined = self.clone();
        combined.write_all(first).ok()?;
        combined.write_all(second).ok()?;
        combined.normalizer.flush_number(&mut combined.hash).ok()?;
        combined.hash.finish(&[], &[])
    }

    pub(super) fn finish_with_suffix(
        &self,
        changed: &[u8],
        suffix: &[u8],
        cached_suffix: &ProjectionHashSuffix,
        exact_digest: [u8; 32],
    ) -> Option<[u8; 32]> {
        if !self.normalizer.at_boundary() {
            return None;
        }
        let mut combined = self.clone();
        combined.write_all(changed).ok()?;
        if !combined.normalizer.at_boundary() {
            return None;
        }
        if !combined.normalizer.changed_bytes && cached_suffix.normalized.is_none() {
            return Some(exact_digest);
        }
        combined
            .hash
            .finish(cached_suffix.normalized.as_deref().unwrap_or(suffix), &[])
    }
}

impl ProjectionHashSuffix {
    pub(super) fn new(bytes: &[u8], byte_budget: usize) -> Option<Self> {
        let mut normalizer = ProjectionNormalizer::default();
        normalizer.write(&mut std::io::sink(), bytes).ok()?;
        if !normalizer.at_boundary() {
            return None;
        }
        if !normalizer.changed_bytes {
            return Some(Self { normalized: None });
        }
        let mut normalized = ProjectionBytes::new(bytes.len(), byte_budget)?;
        ProjectionNormalizer::default()
            .write(&mut normalized, bytes)
            .ok()?;
        Some(Self {
            normalized: Some(normalized.bytes),
        })
    }

    pub(super) fn copy_with_budget(&self, byte_budget: usize) -> Option<Self> {
        let normalized = match &self.normalized {
            Some(bytes) => {
                let mut copied = ProjectionBytes::new(bytes.len(), byte_budget)?;
                copied.write_all(bytes).ok()?;
                Some(copied.bytes)
            }
            None => None,
        };
        Some(Self { normalized })
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.normalized.as_ref().map_or(0, Vec::capacity)
    }
}

struct ProjectionBytes {
    bytes: Vec<u8>,
    byte_budget: usize,
}

impl ProjectionBytes {
    fn new(initial_capacity: usize, byte_budget: usize) -> Option<Self> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(initial_capacity.min(byte_budget))
            .ok()?;
        (bytes.capacity() <= byte_budget).then_some(Self { bytes, byte_budget })
    }
}

impl Write for ProjectionBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next_len = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|len| *len <= self.byte_budget)
            .ok_or_else(|| {
                std::io::Error::other("normalized projection suffix exceeds cache budget")
            })?;
        self.bytes
            .try_reserve_exact(next_len - self.bytes.len())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::OutOfMemory, error))?;
        if self.bytes.capacity() > self.byte_budget {
            return Err(std::io::Error::other(
                "normalized projection suffix capacity exceeds cache budget",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl ProjectionNormalizer {
    fn at_boundary(&self) -> bool {
        !self.in_string && !self.escaped && self.number_len == 0
    }

    fn flush_number(&mut self, output: &mut impl Write) -> std::io::Result<()> {
        if self.number_len == 0 {
            return Ok(());
        }
        let token = &self.number[..self.number_len];
        // Preserve canonical float spellings; reparsing may round distinct values together.
        let float_token = token.iter().any(|byte| matches!(byte, b'.' | b'e' | b'E'));
        let normalized_integer = if float_token {
            None
        } else {
            let number: serde_json::Number = serde_json::from_slice(token)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            let float = if let Some(integer) = number.as_i64() {
                crate::schema::integer_is_exact_binary64(integer.unsigned_abs())
                    .then_some(integer as f64)
            } else {
                number.as_u64().and_then(|integer| {
                    crate::schema::integer_is_exact_binary64(integer).then_some(integer as f64)
                })
            };
            float
                .and_then(serde_json::Number::from_f64)
                .map(|number| number.to_string())
        };
        let normalized = if token == NEGATIVE_ZERO_JSON {
            POSITIVE_ZERO_JSON
        } else {
            normalized_integer
                .as_deref()
                .map(str::as_bytes)
                .unwrap_or(token)
        };
        self.changed_bytes |= normalized != token;
        output.write_all(normalized)?;
        self.number_len = 0;
        Ok(())
    }

    fn push_number_byte(&mut self, byte: u8) -> std::io::Result<()> {
        let slot = self.number.get_mut(self.number_len).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "canonical number exceeds projection token capacity",
            )
        })?;
        *slot = byte;
        self.number_len += 1;
        Ok(())
    }
    fn write(&mut self, output: &mut impl Write, bytes: &[u8]) -> std::io::Result<usize> {
        #[cfg(test)]
        NORMALIZED_INPUT_BYTES.set(NORMALIZED_INPUT_BYTES.get().saturating_add(bytes.len()));
        let mut raw_start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
                continue;
            }
            if self.number_len != 0 {
                if byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-') {
                    self.push_number_byte(byte)?;
                    raw_start = index + 1;
                    continue;
                }
                self.flush_number(output)?;
                raw_start = index;
            }
            if byte == b'"' {
                self.in_string = true;
            } else if byte.is_ascii_digit() || byte == b'-' {
                output.write_all(&bytes[raw_start..index])?;
                self.push_number_byte(byte)?;
                raw_start = index + 1;
            }
        }
        output.write_all(&bytes[raw_start..])?;
        Ok(bytes.len())
    }
}

impl Write for ProjectionHashPrefix {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.normalizer.write(&mut self.hash, bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Write for Sha256Prefix {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.update(bytes)
            .ok_or_else(|| std::io::Error::other("projection SHA-256 update failed"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn projection_sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    ProjectionHashPrefix::new(&[])?.finish(bytes, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cached_suffix_preserves_projection_identity_for_changes_in_each_json_region() {
        const INEXACT_INTEGER: u64 = 9_007_199_254_740_993;
        let text = json!({"text": "😀 7 \\\"-0.0\\\" \\ café"});
        let regions = [
            (text.clone(), text.clone(), text.clone()),
            (json!({"number": 7}), text.clone(), text.clone()),
            (text.clone(), json!({"number": -0.0}), text.clone()),
            (text.clone(), text.clone(), json!({"number": 7})),
            (
                json!({"number": INEXACT_INTEGER}),
                json!({"number": 7.25}),
                json!({"number": u64::MAX}),
            ),
        ];
        for (before, changed, after) in regions {
            let prefix = format!("[{},", before).into_bytes();
            let changed = serde_json::to_vec(&changed).unwrap();
            let suffix = format!(",{}]", after).into_bytes();
            let full = [&prefix[..], &changed[..], &suffix[..]].concat();
            let expected = projection_sha256(&full).unwrap();
            let exact = super::super::canonical_sha256(&full);
            let cached = ProjectionHashSuffix::new(&suffix, usize::MAX).unwrap();
            let prefix = ProjectionHashPrefix::new(&prefix).unwrap();
            take_normalized_input_bytes_for_test();
            assert_eq!(
                prefix.finish_with_suffix(&changed, &suffix, &cached, exact),
                Some(expected),
                "cached regions must preserve full projection identity: {}",
                String::from_utf8_lossy(&full)
            );
            assert_eq!(take_normalized_input_bytes_for_test(), changed.len());
        }
    }

    #[test]
    fn cached_suffix_rejects_non_neutral_json_boundaries() {
        let suffix = br#"}]"#;
        let cached = ProjectionHashSuffix::new(suffix, usize::MAX).unwrap();
        let exact = super::super::canonical_sha256(b"{}");
        for prefix in [&br#"[{"text":""#[..], &br#"[{"text":"\"#[..], &b"[7"[..]] {
            assert!(ProjectionHashPrefix::new(prefix)
                .unwrap()
                .finish_with_suffix(b"{}", suffix, &cached, exact)
                .is_none());
        }
        for changed in [&br#"{"text":""#[..], &br#"{"text":"\"#[..], &b"7"[..]] {
            assert!(ProjectionHashPrefix::new(b"[")
                .unwrap()
                .finish_with_suffix(changed, suffix, &cached, exact)
                .is_none());
        }
        for suffix in [&br#""unfinished"#[..], &br#""escaped\"#[..], &b"7"[..]] {
            assert!(ProjectionHashSuffix::new(suffix, usize::MAX).is_none());
        }
    }

    #[test]
    fn normalized_suffix_expansion_and_copy_stay_within_the_byte_budget() {
        let suffix = br#",{"number":7}]"#;
        let normalized = br#",{"number":7.0}]"#;
        let cached = ProjectionHashSuffix::new(suffix, normalized.len()).unwrap();
        assert_eq!(cached.normalized.as_deref(), Some(&normalized[..]));
        assert_eq!(cached.retained_bytes(), normalized.len());
        assert!(ProjectionHashSuffix::new(suffix, normalized.len() - 1).is_none());
        assert!(cached.copy_with_budget(normalized.len() - 1).is_none());
        assert_eq!(
            cached
                .copy_with_budget(normalized.len())
                .unwrap()
                .normalized,
            cached.normalized
        );
        assert_eq!(
            ProjectionHashSuffix::new(br#",{"text":"7"}]"#, 0)
                .unwrap()
                .retained_bytes(),
            0
        );
    }

    #[test]
    fn projection_hash_matches_numeric_equality_across_every_chunk_boundary() {
        const INEXACT_INTEGER: u64 = 9_007_199_254_740_993;
        const EXACT_LARGE_INTEGER: u64 = 1 << 60;
        let values = [
            json!(7),
            json!(7.0),
            json!(-7),
            json!(-7.0),
            json!(0),
            json!(0.0),
            json!(-0.0),
            json!(7.25),
            json!("7"),
            json!(INEXACT_INTEGER),
            json!(INEXACT_INTEGER as f64),
            json!(EXACT_LARGE_INTEGER),
            json!(EXACT_LARGE_INTEGER as f64),
            json!(i64::MIN),
            json!(i64::MIN as f64),
            json!(u64::MAX),
            json!(u64::MAX as f64),
            json!(f64::MAX),
            json!(-f64::MAX),
            json!(f64::MIN_POSITIVE),
            json!(-f64::MIN_POSITIVE),
            json!(f64::from_bits(1)),
            json!(-f64::from_bits(1)),
            json!(f64::from_bits(0x86dc_510c_7d7d_53d9)),
            json!(f64::from_bits(0x86dc_510c_7d7d_53da)),
        ];
        for left in &values {
            for right in &values {
                let left = json!({"attrs": {"number": [left], "quoted": "😀 7\\\"-0.0 café"}});
                let right = json!({"attrs": {"number": [right], "quoted": "😀 7\\\"-0.0 café"}});
                let bytes = serde_json::to_vec(&left).unwrap();
                let expected = projection_sha256(&serde_json::to_vec(&right).unwrap()).unwrap();
                let equal = crate::boundary::json_projection_values_equal_stack_safe(&left, &right);
                for split in 0..=bytes.len() {
                    let prefix = ProjectionHashPrefix::new(&bytes[..split]).unwrap();
                    assert_eq!(
                        prefix.finish(&bytes[split..], &[]).unwrap() == expected,
                        equal,
                        "projection identity differs at byte {split}: {left} versus {right}"
                    );
                }
            }
        }
    }

    #[test]
    fn deep_projection_hash_preserves_exact_json_identity_and_unicode_strings() {
        const DEPTH: usize = 1024;
        let mut integer = json!({"attrs": {"number": [7, -7, 0], "text": "😀 7 \"-0.0\" \\ café"}});
        let mut float =
            json!({"attrs": {"number": [7.0, -7.0, -0.0], "text": "😀 7 \"-0.0\" \\ café"}});
        for _ in 0..DEPTH {
            integer = json!([integer]);
            float = json!([float]);
        }
        let integer = crate::boundary::StackSafeJsonValue::new(integer);
        let float = crate::boundary::StackSafeJsonValue::new(float);
        let integer_bytes = crate::boundary::serialize_json_value_stack_safe(integer.as_value(), 0);
        let float_bytes = crate::boundary::serialize_json_value_stack_safe(float.as_value(), 0);
        assert_ne!(
            super::super::canonical_sha256(&integer_bytes),
            super::super::canonical_sha256(&float_bytes),
            "exact wire identity must preserve number representation"
        );
        let expected = projection_sha256(&integer_bytes).unwrap();
        let mut streamed = ProjectionHashPrefix::new(&[]).unwrap();
        for byte in &float_bytes {
            streamed.write_all(std::slice::from_ref(byte)).unwrap();
        }
        assert_eq!(
            streamed.finish(&[], &[]),
            Some(expected),
            "single-byte writes must preserve deep arrays, escapes and non-ASCII strings"
        );
    }
}
