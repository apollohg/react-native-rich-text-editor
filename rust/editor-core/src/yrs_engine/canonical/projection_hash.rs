use super::hash_prefix::Sha256Prefix;
use std::io::Write;

const NUMBER_TOKEN_BYTES: usize = 32;
const NEGATIVE_ZERO_JSON: &[u8] = b"-0.0";
const POSITIVE_ZERO_JSON: &[u8] = b"0.0";

#[derive(Clone, Debug)]
pub(super) struct ProjectionHashPrefix {
    hash: Sha256Prefix,
    number: [u8; NUMBER_TOKEN_BYTES],
    number_len: usize,
    in_string: bool,
    escaped: bool,
}

impl ProjectionHashPrefix {
    pub(super) fn new(bytes: &[u8]) -> Option<Self> {
        let mut prefix = Self {
            hash: Sha256Prefix::new(&[])?,
            number: [0; NUMBER_TOKEN_BYTES],
            number_len: 0,
            in_string: false,
            escaped: false,
        };
        prefix.write_all(bytes).ok()?;
        Some(prefix)
    }

    pub(super) fn finish(&self, first: &[u8], second: &[u8]) -> Option<[u8; 32]> {
        let mut combined = self.clone();
        combined.write_all(first).ok()?;
        combined.write_all(second).ok()?;
        combined.flush_number().ok()?;
        combined.hash.finish(&[], &[])
    }

    fn flush_number(&mut self) -> std::io::Result<()> {
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
        self.hash
            .update(normalized)
            .ok_or_else(|| std::io::Error::other("projection SHA-256 update failed"))?;
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
}

impl Write for ProjectionHashPrefix {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
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
                self.flush_number()?;
                raw_start = index;
            }
            if byte == b'"' {
                self.in_string = true;
            } else if byte.is_ascii_digit() || byte == b'-' {
                self.hash
                    .update(&bytes[raw_start..index])
                    .ok_or_else(|| std::io::Error::other("projection SHA-256 update failed"))?;
                self.push_number_byte(byte)?;
                raw_start = index + 1;
            }
        }
        self.hash
            .update(&bytes[raw_start..])
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
