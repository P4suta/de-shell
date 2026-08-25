use serde_json::Value;

pub(crate) fn canonical_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    write_value(value, &mut output, None, 0)?;
    Ok(output)
}

pub(crate) fn pretty_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    write_value(value, &mut output, Some(2), 0)?;
    output.push(b'\n');
    Ok(output)
}

fn write_value(
    value: &Value,
    output: &mut Vec<u8>,
    indent: Option<usize>,
    depth: usize,
) -> Result<(), String> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                output.extend_from_slice(value.to_string().as_bytes());
            } else {
                return Err("canonical contract JSON permits signed 64-bit integers only".into());
            }
        }
        Value::String(value) => write_string(value, output)?,
        Value::Array(values) => {
            output.push(b'[');
            if !values.is_empty() {
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    if let Some(width) = indent {
                        output.push(b'\n');
                        write_indent(output, (depth + 1) * width);
                    }
                    write_value(value, output, indent, depth + 1)?;
                }
                if let Some(width) = indent {
                    output.push(b'\n');
                    write_indent(output, depth * width);
                }
            }
            output.push(b']');
        }
        Value::Object(fields) => {
            output.push(b'{');
            if !fields.is_empty() {
                let mut keys: Vec<&str> = fields.keys().map(String::as_str).collect();
                keys.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    if let Some(width) = indent {
                        output.push(b'\n');
                        write_indent(output, (depth + 1) * width);
                    }
                    write_string(key, output)?;
                    output.push(b':');
                    if indent.is_some() {
                        output.push(b' ');
                    }
                    write_value(&fields[key], output, indent, depth + 1)?;
                }
                if let Some(width) = indent {
                    output.push(b'\n');
                    write_indent(output, depth * width);
                }
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn write_string(value: &str, output: &mut Vec<u8>) -> Result<(), String> {
    let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
    output.extend_from_slice(encoded.as_bytes());
    Ok(())
}

fn write_indent(output: &mut Vec<u8>, width: usize) {
    output.resize(output.len() + width, b' ');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_recursively_without_whitespace() {
        let value = json!({"z": 1, "a": {"y": true, "b": "text"}});
        assert_eq!(
            canonical_bytes(&value).unwrap(),
            br#"{"a":{"b":"text","y":true},"z":1}"#
        );
    }

    #[test]
    fn persisted_json_is_sorted_two_space_indented_and_lf_terminated() {
        let value = json!({"z": 1, "a": [true, {"d": 4, "c": 3}]});
        assert_eq!(
            String::from_utf8(pretty_bytes(&value).unwrap()).unwrap(),
            "{\n  \"a\": [\n    true,\n    {\n      \"c\": 3,\n      \"d\": 4\n    }\n  ],\n  \"z\": 1\n}\n"
        );
    }

    #[test]
    fn canonical_json_rejects_floating_point_values() {
        assert!(canonical_bytes(&json!({"not_in_contract": 1.5})).is_err());
    }
}
