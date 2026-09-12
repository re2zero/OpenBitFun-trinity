//! Numeric rendering for cognitive state payloads.
//!
//! PSI state arrives as raw JSON with full `f64` precision. When the host
//! prints it into model context (or any other text), one decimal place is
//! enough — more digits are false precision and cost tokens.

use serde_json::{Number, Value};

/// Round a JSON value so every floating-point number keeps one decimal place.
///
/// Integers and non-numeric values are passed through unchanged. The result is
/// a compact JSON string.
pub(crate) fn to_json_one_decimal(value: &Value) -> String {
    round_to_one_decimal(value).to_string()
}

/// Round a JSON value so every floating-point number keeps one decimal place.
pub(crate) fn round_to_one_decimal(value: &Value) -> Value {
    match value {
        Value::Number(number) => round_number(number),
        Value::Array(items) => Value::Array(items.iter().map(round_to_one_decimal).collect()),
        Value::Object(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, item)| (key.clone(), round_to_one_decimal(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn round_number(number: &Number) -> Value {
    if number.is_i64() || number.is_u64() {
        return Value::Number(number.clone());
    }
    let Some(raw) = number.as_f64() else {
        return Value::Number(number.clone());
    };
    let rounded = (raw * 10.0).round() / 10.0;
    Number::from_f64(rounded)
        .map(Value::Number)
        .unwrap_or_else(|| Value::Number(number.clone()))
}

#[cfg(test)]
mod tests {
    use super::to_json_one_decimal;
    use serde_json::{json, Value};

    fn parse(value: &Value) -> Value {
        serde_json::from_str(&to_json_one_decimal(value)).expect("rendered JSON must parse")
    }

    #[test]
    fn floats_keep_one_decimal_place() {
        assert_eq!(parse(&json!({ "v": 0.1234 })), json!({ "v": 0.1 }));
        assert_eq!(parse(&json!({ "v": 0.6789 })), json!({ "v": 0.7 }));
        assert_eq!(
            parse(&json!({ "v": 0.9523809523809523 })),
            json!({ "v": 1.0 })
        );
        assert_eq!(parse(&json!({ "v": 0.0 })), json!({ "v": 0.0 }));
        assert_eq!(parse(&json!({ "v": 1.0 })), json!({ "v": 1.0 }));
    }

    #[test]
    fn integers_and_other_values_pass_through() {
        let value = json!({ "cycle": 12, "focus": "explore", "ok": true });
        assert_eq!(parse(&value), value);
    }

    #[test]
    fn nested_state_objects_are_rounded() {
        assert_eq!(
            parse(&json!({
                "needs": { "competence": 0.8765 },
                "history": [0.1234, 0.6789],
            })),
            json!({
                "needs": { "competence": 0.9 },
                "history": [0.1, 0.7],
            })
        );
    }
}
