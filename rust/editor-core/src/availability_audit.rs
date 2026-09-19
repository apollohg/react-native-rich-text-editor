pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_ITEMS: usize = 65_536;

#[cfg(test)]
thread_local! {
    static JSON_SERIALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn freeze_json(value: &serde_json::Value, max_bytes: usize) -> Option<Vec<u8>> {
    JsonBudget::new(max_bytes).observe(value, 0)?;
    #[cfg(test)]
    JSON_SERIALIZATIONS.set(JSON_SERIALIZATIONS.get() + 1);
    let bytes = serde_json::to_vec(value).ok()?;
    (bytes.len() <= max_bytes).then_some(bytes)
}

pub(crate) struct JsonBudget {
    bytes: usize,
    items: usize,
}

impl JsonBudget {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            bytes: max_bytes,
            items: MAX_ITEMS,
        }
    }

    fn charge_string(&mut self, value: &str) -> Option<()> {
        // Covers six-byte JSON escaping, Vec growth and cloned event strings.
        self.bytes = self.bytes.checked_sub(value.len().checked_mul(16)?)?;
        Some(())
    }

    pub(crate) fn observe(&mut self, value: &serde_json::Value, depth: usize) -> Option<()> {
        if depth > 64 {
            return None;
        }
        self.items = self.items.checked_sub(1)?;
        // Includes sparse BTreeMap nodes in both pending-event copies.
        self.bytes = self.bytes.checked_sub(1024)?;
        match value {
            serde_json::Value::String(value) => self.charge_string(value)?,
            serde_json::Value::Array(values) => {
                if values.len() > self.items {
                    return None;
                }
                for value in values {
                    self.observe(value, depth + 1)?;
                }
            }
            serde_json::Value::Object(values) => {
                if values.len() > self.items {
                    return None;
                }
                for (key, value) in values {
                    self.charge_string(key)?;
                    self.observe(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_raw_content_limit_stops_before_serialization() {
        let value = serde_json::json!({"text": "x".repeat(32768)});
        JSON_SERIALIZATIONS.set(0);
        assert!(freeze_json(&value, 16384).is_none());
        assert_eq!(JSON_SERIALIZATIONS.get(), 0);
    }
}
