use crate::Commands;
use agent_desktop_core::AppError;
use std::io::{IsTerminal, Read};

const MAX_STDIN_BYTES: usize = 128 * 1024;
const MAX_BATCH_STDIN_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StdinRedaction {
    None,
    AllValues,
    BatchEntries,
}

pub(crate) fn redaction(command: &Commands) -> StdinRedaction {
    match command {
        Commands::Type(args) if args.stdin => StdinRedaction::AllValues,
        Commands::SetValue(args) if args.stdin => StdinRedaction::AllValues,
        Commands::ClipboardSet(args) if args.stdin => StdinRedaction::AllValues,
        Commands::Batch(args) if args.stdin => StdinRedaction::BatchEntries,
        _ => StdinRedaction::None,
    }
}

pub(crate) fn resolve_stdin_text(
    command: &mut Commands,
    reader: impl Read + IsTerminal,
) -> Result<(), AppError> {
    let is_batch = matches!(command, Commands::Batch(args) if args.stdin);
    if let Some(value) = input(command) {
        if reader.is_terminal() {
            return Err(AppError::invalid_input(
                "--stdin requires a pipe or redirected file",
            ));
        }
        *value = Some(if is_batch {
            read_batch_json(reader)?
        } else {
            read_text(reader)?
        });
    }
    Ok(())
}

pub(crate) fn input(command: &mut Commands) -> Option<&mut Option<String>> {
    match command {
        Commands::Type(args) if args.stdin => Some(&mut args.text),
        Commands::SetValue(args) if args.stdin => Some(&mut args.value),
        Commands::ClipboardSet(args) if args.stdin => Some(&mut args.text),
        Commands::Batch(args) if args.stdin => Some(&mut args.commands_json),
        _ => None,
    }
}

pub(crate) fn redact_result(
    mut result: Result<serde_json::Value, AppError>,
    redaction: StdinRedaction,
) -> Result<serde_json::Value, AppError> {
    match redaction {
        StdinRedaction::None => {}
        StdinRedaction::AllValues => {
            let data = match &mut result {
                Ok(data) => Some(data),
                Err(AppError::Adapter(error)) => error.details.as_mut(),
                _ => None,
            };
            if let Some(data) = data {
                omit_values(data);
            }
        }
        StdinRedaction::BatchEntries => match &mut result {
            Ok(data) => omit_batch_entry_values(data),
            Err(AppError::Adapter(error)) => {
                if let Some(details) = error.details.as_mut() {
                    omit_values(details);
                }
            }
            _ => {}
        },
    }
    result
}

fn omit_batch_entry_values(data: &mut serde_json::Value) {
    let Some(entries) = data
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let has_text_write = entries.iter().any(|entry| {
        entry
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(is_text_write_command)
    });
    for entry in entries {
        let command = entry.get("command").and_then(serde_json::Value::as_str);
        if command.is_some_and(is_text_write_command) {
            if let Some(entry_data) = entry.get_mut("data") {
                omit_values(entry_data);
            }
            if let Some(details) = entry
                .get_mut("error")
                .and_then(|error| error.get_mut("details"))
            {
                omit_values(details);
            }
        } else if has_text_write && !command.is_some_and(is_explicit_read_command) {
            if let Some(post_state) = entry.get_mut("data").and_then(|d| d.get_mut("post_state")) {
                omit_values(post_state);
            }
            if let Some(post_state) = entry
                .get_mut("error")
                .and_then(|e| e.get_mut("details"))
                .and_then(|d| d.get_mut("post_state"))
            {
                omit_values(post_state);
            }
        }
    }
}

fn is_text_write_command(command: &str) -> bool {
    matches!(command, "type" | "set-value" | "clipboard-set")
}

fn is_explicit_read_command(command: &str) -> bool {
    matches!(command, "get" | "clipboard-get")
}

fn omit_values(data: &mut serde_json::Value) {
    match data {
        serde_json::Value::Object(fields) => {
            fields.remove("value");
            fields.remove("text");
            fields.remove("stdin");
            fields.values_mut().for_each(omit_values);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(omit_values),
        _ => {}
    }
}

pub(crate) fn read_text(reader: impl Read) -> Result<String, AppError> {
    read_bounded_utf8(
        reader,
        MAX_STDIN_BYTES,
        "stdin text exceeds the 128 KiB limit",
    )
}

pub(crate) fn read_batch_json(reader: impl Read) -> Result<String, AppError> {
    let text = read_bounded_utf8(
        reader,
        MAX_BATCH_STDIN_BYTES,
        "stdin batch JSON exceeds the 1 MiB limit",
    )?;
    if text.trim().is_empty() {
        return Err(AppError::invalid_input("stdin batch JSON cannot be empty"));
    }
    Ok(text)
}

fn read_bounded_utf8(
    reader: impl Read,
    max_bytes: usize,
    limit_message: &'static str,
) -> Result<String, AppError> {
    let mut bytes = Vec::new();
    reader
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| AppError::invalid_input("Could not read text from stdin"))?;
    validate_bounded_bytes(&bytes, max_bytes, limit_message)?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::invalid_input("stdin must contain valid UTF-8 text"))
}

fn validate_bounded_bytes(
    bytes: &[u8],
    max_bytes: usize,
    limit_message: &'static str,
) -> Result<(), AppError> {
    if bytes.len() > max_bytes {
        return Err(AppError::invalid_input(limit_message));
    }
    if bytes.contains(&0) {
        return Err(AppError::invalid_input(
            "stdin text cannot contain NUL bytes",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "stdin_tests.rs"]
mod tests;
