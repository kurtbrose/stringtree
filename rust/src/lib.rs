use indexmap::IndexMap;
use regex::Regex;
use serde::ser::{Serialize, Serializer};
use thiserror::Error;

static KEY_PATTERN: &str = r"^[A-Za-z_][A-Za-z0-9_]*$";

#[derive(Debug, Error, PartialEq, Eq)]
#[error("line {line}, column {column}: {message}")]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IstValue {
    String(String),
    Object(IndexMap<String, IstValue>),
    Array(Vec<IstValue>),
}

impl Serialize for IstValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            IstValue::String(v) => serializer.serialize_str(v),
            IstValue::Object(map) => {
                let mut state = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map.iter() {
                    state.serialize_entry(k, v)?;
                }
                state.end()
            }
            IstValue::Array(items) => items.serialize(serializer),
        }
    }
}

enum ContainerType {
    Object,
    Array,
}

pub fn parse(source: &str) -> Result<IstValue, ParseError> {
    let regex = Regex::new(KEY_PATTERN).expect("valid regex");
    let mut processed: Vec<(usize, String)> = Vec::new();
    for (lineno, line) in source.split('\n').enumerate() {
        let ln = lineno + 1;
        let trimmed = line.trim_end_matches('\n');
        ensure_no_tabs(trimmed, ln)?;
        ensure_no_trailing_whitespace(trimmed, ln)?;
        processed.push((ln, trimmed.to_string()));
    }

    let mut next_index = next_significant(&processed, 0);
    if next_index.is_none() {
        return Err(ParseError {
            line: 1,
            column: 1,
            message: "document is empty".to_string(),
        });
    }
    let start = next_index.unwrap();
    let (first_lineno, first_raw) = &processed[start];
    let first_indent = leading_indent(first_raw, *first_lineno)?;
    if first_indent != 0 {
        return Err(ParseError {
            line: *first_lineno,
            column: 1,
            message: "document must start at indentation level 0".to_string(),
        });
    }

    let first_text = first_raw.trim_start();
    let container_type = if first_text.starts_with('-') {
        ContainerType::Array
    } else if first_text.contains(':') {
        ContainerType::Object
    } else {
        return Err(ParseError {
            line: *first_lineno,
            column: 1,
            message: "root must be an object or array entry".to_string(),
        });
    };

    let (parsed, consumed) = parse_container(&processed, start, 0, &container_type, &regex)?;

    next_index = next_significant(&processed, consumed);
    if let Some(idx) = next_index {
        let (lineno, _) = processed[idx];
        return Err(ParseError {
            line: lineno,
            column: 1,
            message: "unexpected content after top-level value".to_string(),
        });
    }

    Ok(parsed)
}

fn parse_container(
    lines: &[(usize, String)],
    mut index: usize,
    indent_level: usize,
    container_type: &ContainerType,
    key_regex: &Regex,
) -> Result<(IstValue, usize), ParseError> {
    match container_type {
        ContainerType::Object => {
            let mut container: IndexMap<String, IstValue> = IndexMap::new();
            while index < lines.len() {
                let (lineno, raw) = &lines[index];
                if raw.trim().is_empty() {
                    index += 1;
                    continue;
                }
                let indent = leading_indent(raw, *lineno)?;
                let stripped = &raw[indent * 2..];
                if stripped.trim_start().starts_with('#') {
                    index += 1;
                    continue;
                }
                if indent < indent_level {
                    break;
                }
                if indent > indent_level {
                    return Err(ParseError {
                        line: *lineno,
                        column: 1,
                        message: "indentation may only increase by one level at a time".to_string(),
                    });
                }

                let (key, value, consumed) =
                    parse_object_entry(lines, index, indent_level, key_regex)?;
                if container.contains_key(&key) {
                    return Err(ParseError {
                        line: *lineno,
                        column: 1,
                        message: format!("duplicate key '{}' at this object level", key),
                    });
                }
                container.insert(key, value);
                index = consumed;
            }
            Ok((IstValue::Object(container), index))
        }
        ContainerType::Array => {
            let mut items: Vec<IstValue> = Vec::new();
            while index < lines.len() {
                let (lineno, raw) = &lines[index];
                if raw.trim().is_empty() {
                    index += 1;
                    continue;
                }
                let indent = leading_indent(raw, *lineno)?;
                let stripped = &raw[indent * 2..];
                if stripped.trim_start().starts_with('#') {
                    index += 1;
                    continue;
                }
                if indent < indent_level {
                    break;
                }
                if indent > indent_level {
                    return Err(ParseError {
                        line: *lineno,
                        column: 1,
                        message: "indentation may only increase by one level at a time".to_string(),
                    });
                }

                let (value, consumed) = parse_array_entry(lines, index, indent_level, key_regex)?;
                items.push(value);
                index = consumed;
            }
            Ok((IstValue::Array(items), index))
        }
    }
}

fn parse_object_entry(
    lines: &[(usize, String)],
    index: usize,
    indent_level: usize,
    key_regex: &Regex,
) -> Result<(String, IstValue, usize), ParseError> {
    let (lineno, raw) = &lines[index];
    let stripped = &raw[indent_level * 2..];
    let (key_segment, value_segment) = stripped.split_once(':').ok_or_else(|| ParseError {
        line: *lineno,
        column: 1,
        message: "object entries must contain a ':' separator".to_string(),
    })?;

    if !key_regex.is_match(key_segment) {
        return Err(ParseError {
            line: *lineno,
            column: 1,
            message: "object keys must match [A-Za-z_][A-Za-z0-9_]*".to_string(),
        });
    }

    if value_segment.is_empty() {
        let next_sig = next_significant(lines, index + 1);
        if next_sig.is_none() {
            return Ok((
                key_segment.to_string(),
                IstValue::String(String::new()),
                lines.len(),
            ));
        }
        let next_idx = next_sig.unwrap();
        let (next_lineno, next_raw) = &lines[next_idx];
        let next_indent = leading_indent(next_raw, *next_lineno)?;
        if next_indent == indent_level {
            return Ok((
                key_segment.to_string(),
                IstValue::String(String::new()),
                next_idx,
            ));
        }
        if next_indent != indent_level + 1 {
            return Err(ParseError {
                line: *next_lineno,
                column: 1,
                message: "indentation must increase by exactly one level for a block".to_string(),
            });
        }
        let next_stripped = &next_raw[next_indent * 2..];
        let nested_type = if next_stripped.trim_start().starts_with('-') {
            ContainerType::Array
        } else if next_stripped.contains(':') {
            ContainerType::Object
        } else {
            return Err(ParseError {
                line: *next_lineno,
                column: 1,
                message: "block must start with an object or array entry".to_string(),
            });
        };
        let (nested, consumed) =
            parse_container(lines, next_idx, indent_level + 1, &nested_type, key_regex)?;
        return Ok((key_segment.to_string(), nested, consumed));
    }

    if !value_segment.starts_with(' ') {
        return Err(ParseError {
            line: *lineno,
            column: raw.len(),
            message: "inline string values must follow a space".to_string(),
        });
    }

    Ok((
        key_segment.to_string(),
        IstValue::String(value_segment[1..].to_string()),
        index + 1,
    ))
}

fn parse_array_entry(
    lines: &[(usize, String)],
    index: usize,
    indent_level: usize,
    key_regex: &Regex,
) -> Result<(IstValue, usize), ParseError> {
    let (lineno, raw) = &lines[index];
    let stripped = &raw[indent_level * 2..];
    if !stripped.starts_with('-') {
        return Err(ParseError {
            line: *lineno,
            column: 1,
            message: "array entries must start with '-'".to_string(),
        });
    }
    let payload = &stripped[1..];
    if payload.is_empty() {
        let next_sig = next_significant(lines, index + 1);
        if next_sig.is_none() {
            return Ok((IstValue::String(String::new()), lines.len()));
        }
        let next_idx = next_sig.unwrap();
        let (next_lineno, next_raw) = &lines[next_idx];
        let next_indent = leading_indent(next_raw, *next_lineno)?;
        if next_indent == indent_level {
            return Ok((IstValue::String(String::new()), next_idx));
        }
        if next_indent != indent_level + 1 {
            return Err(ParseError {
                line: *next_lineno,
                column: 1,
                message: "indentation must increase by exactly one level for a block".to_string(),
            });
        }
        let next_stripped = &next_raw[next_indent * 2..];
        let nested_type = if next_stripped.trim_start().starts_with('-') {
            ContainerType::Array
        } else if next_stripped.contains(':') {
            ContainerType::Object
        } else {
            return Err(ParseError {
                line: *next_lineno,
                column: 1,
                message: "block must start with an object or array entry".to_string(),
            });
        };
        let (nested, consumed) =
            parse_container(lines, next_idx, indent_level + 1, &nested_type, key_regex)?;
        return Ok((nested, consumed));
    }

    if !payload.starts_with(' ') {
        return Err(ParseError {
            line: *lineno,
            column: raw.len(),
            message: "inline string values must follow a space".to_string(),
        });
    }

    Ok((IstValue::String(payload[1..].to_string()), index + 1))
}

pub fn validate_semantics(value: &IstValue) -> Result<(), String> {
    match value {
        IstValue::String(_) => Ok(()),
        IstValue::Object(map) => {
            for (key, nested) in map.iter() {
                if key.is_empty() {
                    return Err("object keys must be strings".to_string());
                }
                validate_semantics(nested)?;
            }
            Ok(())
        }
        IstValue::Array(items) => {
            for item in items.iter() {
                validate_semantics(item)?;
            }
            Ok(())
        }
    }
}

pub fn to_canonical_json(value: &IstValue) -> Result<String, serde_json::Error> {
    let mut serializer = serde_json::Serializer::new(Vec::new());
    value.serialize(&mut serializer)?;
    let bytes = serializer.into_inner();
    let string = String::from_utf8(bytes).expect("serializer emits utf-8");
    Ok(string)
}

fn ensure_no_tabs(line: &str, lineno: usize) -> Result<(), ParseError> {
    if let Some(pos) = line.find('\t') {
        return Err(ParseError {
            line: lineno,
            column: pos + 1,
            message: "tabs are not allowed".to_string(),
        });
    }
    Ok(())
}

fn ensure_no_trailing_whitespace(line: &str, lineno: usize) -> Result<(), ParseError> {
    if line.trim_end_matches(' ') != line {
        return Err(ParseError {
            line: lineno,
            column: line.len(),
            message: "trailing whitespace is not allowed".to_string(),
        });
    }
    Ok(())
}

fn leading_indent(line: &str, lineno: usize) -> Result<usize, ParseError> {
    let spaces = line.len() - line.trim_start_matches(' ').len();
    if spaces % 2 != 0 {
        return Err(ParseError {
            line: lineno,
            column: spaces + 1,
            message: "indentation must use multiples of two spaces".to_string(),
        });
    }
    Ok(spaces / 2)
}

fn next_significant(lines: &[(usize, String)], start: usize) -> Option<usize> {
    for idx in start..lines.len() {
        let (_, text) = &lines[idx];
        if text.trim().is_empty() {
            continue;
        }
        if text.trim_start().starts_with('#') {
            continue;
        }
        return Some(idx);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_structures() {
        let source = r#"
        title: Demo
        items:
          - first
          - second
        metadata:
          author:
            name: Ada
        "#
        .trim();

        let parsed = parse(source).expect("parses");
        match parsed {
            IstValue::Object(root) => {
                let keys: Vec<_> = root.keys().cloned().collect();
                assert_eq!(keys, vec!["title", "items", "metadata"]);
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn rejects_bad_indentation() {
        let source = r#"
        root:
          - child: value
            subchild: nope
        "#
        .trim();

        let err = parse(source).expect_err("should fail");
        assert!(err
            .to_string()
            .contains("indentation may only increase by one level"));
    }

    #[test]
    fn canonical_json_preserves_ordering() {
        let source = r#"
        name: Example
        data:
          - alpha
          - beta
        "#
        .trim();

        let parsed = parse(source).expect("parses");
        let json = to_canonical_json(&parsed).expect("serializes");
        assert!(json.contains("\"name\":\"Example\""));
        assert!(json.contains("\"data\":[\"alpha\",\"beta\"]"));
    }
}
