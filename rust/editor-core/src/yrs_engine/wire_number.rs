use serde_json::Number;
use yrs::any::Any;

use crate::schema::integer_is_exact_binary64;

pub(crate) enum WireNumberError {
    ExceedsExactIntegerRange,
    NotFinite,
    NotRepresentable,
}

pub(crate) fn wire_number_to_any(number: &Number) -> Result<Any, WireNumberError> {
    if let Some(value) = number.as_i64() {
        return integer_is_exact_binary64(value.unsigned_abs())
            .then(|| Any::Number(value as f64))
            .ok_or(WireNumberError::ExceedsExactIntegerRange);
    }
    if let Some(value) = number.as_u64() {
        return integer_is_exact_binary64(value)
            .then(|| Any::Number(value as f64))
            .ok_or(WireNumberError::ExceedsExactIntegerRange);
    }
    match number.as_f64() {
        Some(value) if value.is_finite() => Ok(Any::Number(value)),
        Some(_) => Err(WireNumberError::NotFinite),
        None => Err(WireNumberError::NotRepresentable),
    }
}
