//! The assistant: JSON, and Claude's requests and answers.

use crate::web::claude::{self, Turn};
use crate::web::json::{self, Value};

#[test]
fn json_parses_and_quotes() {
    let v = json::parse(r#"{"a": [1, -2.5e1, true, null], "s": "x\"y\\z\né😀", "o": {}}"#).unwrap();
    assert_eq!(v.get("a").arr().len(), 4);
    assert_eq!(v.get("a").arr()[1].num(), Some(-25.0));
    assert_eq!(v.get("a").arr()[2], Value::Bool(true));
    assert_eq!(v.get("s").str(), Some("x\"y\\z\né😀"));
    assert_eq!(v.get("missing"), &Value::Null);
    for bad in ["", "{", "[1,]", "{\"a\" 1}", "\"\u{1}\"", "tru", "[1] x"] {
        assert!(json::parse(bad).is_none(), "{:?}", bad);
    }
    // what quote writes, parse reads back
    let s = "line one\nline \"two\"\t\\ \u{7} é";
    assert_eq!(json::parse(&json::quote(s)).unwrap().str(), Some(s));
    // deep nesting is refused, not a stack overflow
    assert!(json::parse(&"[".repeat(100_000)).is_none());
}

#[test]
fn claude_request_body() {
    let turns = vec![
        Turn { user: false, text: "left over".into() },
        Turn { user: true, text: "Hi \"Claude\"".into() },
        Turn { user: false, text: "Hello!".into() },
        Turn { user: true, text: "Plan a dinner".into() },
    ];
    let body = String::from_utf8(claude::request("some-model", "be brief", &turns)).unwrap();
    let v = json::parse(&body).unwrap();
    assert_eq!(v.get("model").str(), Some("some-model"));
    assert_eq!(v.get("system").str(), Some("be brief"));
    assert_eq!(v.get("max_tokens").num(), Some(claude::MAX_TOKENS as f64));
    let msgs = v.get("messages").arr();
    // it starts with the person's turn and alternates
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].get("role").str(), Some("user"));
    assert_eq!(msgs[0].get("content").str(), Some("Hi \"Claude\""));
    assert_eq!(msgs[1].get("role").str(), Some("assistant"));
    assert_eq!(msgs[2].get("content").str(), Some("Plan a dinner"));
    // long conversations keep the newest turns
    let many: Vec<Turn> = (0..200).map(|i| Turn { user: i % 2 == 0, text: format!("t{}", i) }).collect();
    let v = json::parse(&String::from_utf8(claude::request("m", "", &many)).unwrap()).unwrap();
    let msgs = v.get("messages").arr();
    assert!(msgs.len() <= claude::MAX_TURNS);
    assert_eq!(msgs[0].get("role").str(), Some("user"));
    assert_eq!(msgs.last().unwrap().get("content").str(), Some("t199"));
}

#[test]
fn claude_answers_and_errors() {
    let ok = br###"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"text","text":"## Dinner\n**Monday**: soup\n* bread"}],"stop_reason":"end_turn"}"###;
    let a = claude::answer(200, ok).unwrap();
    assert_eq!(a.text, "Dinner\nMonday: soup\n- bread");
    assert_eq!(a.note, None);
    let cut = br#"{"content":[{"type":"text","text":"Once upon"}],"stop_reason":"max_tokens"}"#;
    assert!(claude::answer(200, cut).unwrap().note.is_some());
    let refused = br#"{"content":[],"stop_reason":"refusal"}"#;
    let r = claude::answer(200, refused).unwrap();
    assert!(r.text.is_empty() && r.note.is_some());
    let e = br#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
    assert!(claude::answer(401, e).unwrap_err().contains("API key"));
    let e = br#"{"type":"error","error":{"type":"invalid_request_error","message":"messages: text content blocks must be non-empty"}}"#;
    assert!(claude::answer(400, e).unwrap_err().contains("must be non-empty"));
    assert!(claude::answer(529, b"").unwrap_err().contains("busy"));
    assert!(claude::answer(200, b"not json").is_err());
}

#[test]
fn claude_models_and_keys() {
    let list = br#"{"data":[{"id":"claude-sonnet-x","display_name":"Claude Sonnet X"},{"id":"claude-opus-y","display_name":"Claude Opus Y"},{"id":"claude-opus-z","display_name":"Claude Opus Z"}],"has_more":false}"#;
    let m = claude::models(200, list).unwrap();
    assert_eq!(m.len(), 3);
    // the newest Opus (the list is newest first)
    assert_eq!(claude::pick(&m).unwrap().0, "claude-opus-y");
    assert_eq!(claude::pick(&m[..1]).unwrap().0, "claude-sonnet-x");
    assert!(claude::models(200, br#"{"data":[]}"#).is_err());
    assert!(claude::models(401, b"{}").is_err());

    assert!(claude::key_ok("  sk-ant-api03-AbC_def-123456789  "));
    assert!(!claude::key_ok("sk-ant-short"));
    assert!(!claude::key_ok("sk-proj-abcdefghijklmnopqrstu"));
    assert!(!claude::key_ok("sk-ant-api03 with spaces inside"));
    assert_eq!(claude::key_hint("sk-ant-api03-secretsecret-WXYZ"), "sk-ant-…WXYZ");
    let h = claude::headers(" sk-ant-k ");
    assert!(h.contains(&("x-api-key".into(), "sk-ant-k".into())));
    assert!(h.iter().any(|(k, _)| k == "anthropic-version"));
    // the system prompt names Claude and the person
    let s = claude::system_prompt("Ada Obi", "Tuesday 29 September 2026");
    assert!(s.contains("You are Claude, made by Anthropic") && s.contains("Ada Obi") && s.contains("29 September"));
}

#[test]
fn api_headers_reach_the_request() {
    use crate::web::{http, url};
    let extra = vec![("x-api-key".to_string(), "sk-ant-k".to_string()), ("bad\r\nx".to_string(), "v".to_string()), ("x-evil".to_string(), "a\r\nInjected: 1".to_string())];
    let req = String::from_utf8(http::request("POST", &url::Url::parse("https://api.anthropic.com/v1/messages").unwrap(), b"{}", "application/json", "", "application/json", "", &extra)).unwrap();
    assert!(req.contains("\r\nx-api-key: sk-ant-k\r\n"));
    assert!(req.contains("Content-Length: 2\r\n"));
    assert!(!req.contains("Injected"));
    assert!(!req.contains("bad"));
}
