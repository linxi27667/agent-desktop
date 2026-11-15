use crate::AppError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchCommand {
    pub command: String,
    pub session: Option<String>,
    #[serde(default)]
    pub args: Value,
}

pub fn sanitize_serde_diagnostic(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            out.push_str("\"<redacted>\"");
            let mut escaped = false;
            for inner in chars.by_ref() {
                if escaped {
                    escaped = false;
                } else if inner == '\\' {
                    escaped = true;
                } else if inner == '"' {
                    break;
                }
            }
        } else {
            out.push(ch);
            if out.ends_with("unknown variant ") && chars.peek() == Some(&'`') {
                let _ = chars.next();
                out.push_str("`<redacted>`");
                for inner in chars.by_ref() {
                    if inner == '`' {
                        break;
                    }
                }
            }
        }
    }
    out
}

pub fn parse_commands(json_str: &str) -> Result<Vec<BatchCommand>, AppError> {
    serde_json::from_str(json_str).map_err(|e| {
        AppError::invalid_input(format!(
            "Invalid batch JSON: {}",
            sanitize_serde_diagnostic(&e.to_string())
        ))
    })
}
