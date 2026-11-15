use super::{StdinRedaction, read_batch_json, read_text, redact_result, redaction};
use serde_json::json;
use std::io::Cursor;

#[test]
fn stdin_text_preserves_empty_input_unicode_crlf_and_trailing_newlines() {
    assert_eq!(read_text(Cursor::new(b"")).unwrap(), "");
    let text = "first\r\nsecond\n世界";
    assert_eq!(read_text(Cursor::new(text.as_bytes())).unwrap(), text);
}

#[test]
fn stdin_text_accepts_the_limit_and_rejects_one_extra_byte() {
    let input = vec![b'x'; 128 * 1024];
    assert_eq!(read_text(Cursor::new(input)).unwrap().len(), 128 * 1024);
    let error = read_text(Cursor::new(vec![b'x'; 128 * 1024 + 1])).unwrap_err();
    assert!(error.to_string().contains("128 KiB"));
}

#[test]
fn batch_stdin_accepts_one_mib_and_rejects_empty_extra_byte_or_nul() {
    for blank in [b"".as_slice(), b"   \r\n\t  "] {
        let empty_err = read_batch_json(Cursor::new(blank)).unwrap_err();
        assert_eq!(empty_err.code(), "INVALID_ARGS");
    }
    let mut input = vec![b' '; 1024 * 1024];
    input[0] = b'[';
    input[1024 * 1024 - 1] = b']';
    assert_eq!(
        read_batch_json(Cursor::new(input)).unwrap().len(),
        1024 * 1024
    );
    let oversized = read_batch_json(Cursor::new(vec![b' '; 1024 * 1024 + 1])).unwrap_err();
    assert_eq!(oversized.code(), "INVALID_ARGS");
    assert!(oversized.to_string().contains("1 MiB"));
    let nul = read_batch_json(Cursor::new(b"[{\"secret\":\"private-marker\0\"}]")).unwrap_err();
    assert_eq!(nul.code(), "INVALID_ARGS");
    assert!(!nul.to_string().contains("private-marker"));
}

#[test]
fn batch_stdin_malformed_json_does_not_echo_confidential_values() {
    let raw = r#"[{"command":"set-value","args":{"ref_id":"@s8f3k2p9:e1","value":"private-marker"#;
    let error = agent_desktop_core::commands::batch::parse_commands(raw).unwrap_err();
    assert_eq!(error.code(), "INVALID_ARGS");
    assert!(!error.to_string().contains("private-marker"));

    let bad_arg = crate::batch::parse_command(agent_desktop_core::commands::batch::BatchCommand {
        command: "snapshot".to_string(),
        session: None,
        args: json!({"surface": "private-marker-surface"}),
    })
    .unwrap_err();
    assert_eq!(bad_arg.code(), "INVALID_ARGS");
    assert!(!bad_arg.to_string().contains("private-marker-surface"));
}

#[test]
fn stdin_text_rejects_invalid_utf8_and_nul_without_echoing_input() {
    for input in [b"private-marker\xff".as_slice(), b"private-marker\0"] {
        let error = read_text(Cursor::new(input)).unwrap_err();
        assert_eq!(error.code(), "INVALID_ARGS");
        assert!(!error.to_string().contains("private-marker"));
    }
}

#[test]
fn stdin_read_failures_are_generic() {
    struct Failure;

    impl std::io::Read for Failure {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("private marker"))
        }
    }

    let error = read_text(Failure).unwrap_err();
    assert_eq!(error.code(), "INVALID_ARGS");
    assert!(!error.to_string().contains("private marker"));
}

#[test]
fn stdin_flags_conflict_with_other_input_sources() {
    use clap::Parser;
    for args in [
        vec!["ad", "type", "@s8f3k2p9:e1"],
        vec!["ad", "set-value", "@s8f3k2p9:e1"],
        vec!["ad", "clipboard-set"],
        vec!["ad", "batch"],
    ] {
        let parse = |extra: &[&str]| crate::Cli::try_parse_from(args.iter().chain(extra).copied());
        let mut command = parse(&["--stdin"]).unwrap().command.unwrap();
        let expected_redaction = if args[1] == "batch" {
            StdinRedaction::BatchEntries
        } else {
            StdinRedaction::AllValues
        };
        assert_eq!(redaction(&command), expected_redaction);
        assert!(super::input(&mut command).is_some());
        assert!(parse(&["text", "--stdin"]).is_err());
        let mut literal = parse(&["--", "--stdin"]).unwrap().command.unwrap();
        assert_eq!(redaction(&literal), StdinRedaction::None);
        assert!(super::input(&mut literal).is_none());
        if args[1] == "clipboard-set" {
            for flag in ["--image", "--file-url"] {
                assert!(parse(&["--stdin", flag, "file"]).is_err());
            }
        } else {
            assert!(parse(&[]).is_err());
        }
    }
}

#[test]
fn batch_stdin_redacts_write_entry_values_while_preserving_readback_values() {
    let payload = json!({
        "results": [
            {
                "command": "set-value",
                "ok": true,
                "data": {"post_state": {"value": "secret-1", "enabled": true}}
            },
            {
                "command": "type",
                "ok": false,
                "error": {"code": "ACTION_FAILED", "details": {"value": "secret-2", "step": 1}}
            },
            {
                "command": "clipboard-set",
                "ok": true,
                "data": {"text": "secret-3", "copied": true}
            },
            {
                "command": "click",
                "ok": true,
                "data": {"post_state": {"value": "secret-1", "focused": true}}
            },
            {
                "command": "get",
                "ok": true,
                "data": {"property": "value", "value": "visible-readback"}
            }
        ]
    });
    let redacted = redact_result(Ok(payload), StdinRedaction::BatchEntries).unwrap();
    let entries = redacted["results"].as_array().unwrap();
    assert!(entries[0]["data"]["post_state"].get("value").is_none());
    assert_eq!(entries[0]["data"]["post_state"]["enabled"], true);
    assert!(entries[1]["error"]["details"].get("value").is_none());
    assert_eq!(entries[1]["error"]["details"]["step"], 1);
    assert!(entries[2]["data"].get("text").is_none());
    assert_eq!(entries[2]["data"]["copied"], true);
    assert!(entries[3]["data"]["post_state"].get("value").is_none());
    assert_eq!(entries[3]["data"]["post_state"]["focused"], true);
    assert_eq!(entries[4]["data"]["value"], "visible-readback");
}
