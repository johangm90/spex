use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub(super) fn required_str<'a>(args: &'a Value, field: &str) -> Result<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Missing field: {}", field))
}

pub(super) fn optional_str<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(Value::as_str)
}

pub(super) fn optional_i64(args: &Value, field: &str) -> Option<i64> {
    args.get(field).and_then(Value::as_i64)
}

pub(super) fn optional_bool(args: &Value, field: &str) -> Option<bool> {
    args.get(field).and_then(Value::as_bool)
}

pub(super) fn string_array(args: &Value, field: &str) -> Result<Vec<String>> {
    let Some(value) = args.get(field) else {
        return Ok(Vec::new());
    };

    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!("{} must be a JSON array of strings", field))?;

    arr.iter()
        .enumerate()
        .map(|(i, item)| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("{}[{}] must be a string", field, i))
        })
        .collect()
}

pub(super) fn related_to_json(args: &Value) -> Result<Option<String>> {
    let Some(value) = args.get("related_to") else {
        return Ok(None);
    };

    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!("related_to must be a JSON array of strings"))?;

    for (i, item) in arr.iter().enumerate() {
        let s = item
            .as_str()
            .ok_or_else(|| anyhow!("related_to[{i}] must be a string"))?;
        if !s.contains('/') {
            bail!("related_to[{i}] must be in 'agent/key' format, got: {s}");
        }
    }

    Ok(Some(value.to_string()))
}
