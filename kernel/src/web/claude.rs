//! Claude, HydatekOS's assistant: requests to and answers from the
//! Anthropic API (the Messages and Models endpoints). The network work is
//! done by the fetcher like any other web request; this file only builds
//! the requests and reads the answers, so the host tests cover it.

use super::json::{self, quote, Value};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=100";
pub const API_VERSION: &str = "2023-06-01";
/// Longest answer asked for (tokens).
pub const MAX_TOKENS: u32 = 16000;
/// Long answers take a while: wait up to five minutes for one.
pub const PATIENCE_MS: u64 = 300_000;
/// Oldest turns are left out beyond this many.
pub const MAX_TURNS: usize = 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    /// said by the person (otherwise by Claude)
    pub user: bool,
    pub text: String,
}

/// The headers every request carries.
pub fn headers(key: &str) -> Vec<(String, String)> {
    alloc::vec![(String::from("x-api-key"), String::from(key.trim())), (String::from("anthropic-version"), String::from(API_VERSION))]
}

/// Does this look like an Anthropic API key?
pub fn key_ok(key: &str) -> bool {
    let k = key.trim();
    k.starts_with("sk-ant-") && k.len() >= 20 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// "sk-ant-…a1b2": enough to recognise a key without showing it.
pub fn key_hint(key: &str) -> String {
    let k = key.trim();
    let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("sk-ant-…{}", tail)
}

/// What Claude is told about where it is.
pub fn system_prompt(name: &str, date: &str) -> String {
    let who = if name.is_empty() { String::from("The person you're talking with hasn't given their name.") } else { format!("The person you're talking with is {}.", name) };
    format!(
        "You are Claude, made by Anthropic, the assistant built into HydatekOS, a desktop and mobile operating system. {} Today is {}. \
Your answers appear as plain text in a chat window that doesn't show Markdown: write in short paragraphs, use simple lists starting with \"- \" when they help, \
and don't use headings, tables, bold or code fences. HydatekOS has these apps: Files, Browser, Messages, Mail, Calendar, Notes, Hyda Scripts (documents), \
Hyda Grids (spreadsheets), Hyda Slides (presentations), Music, Settings, Terminal and Phone Link. You can't operate them or see the screen; when someone asks \
how to do something in HydatekOS, explain the steps. The Gen key (the Ctrl key, or Command on a Mac) runs shortcuts such as Gen+C to copy, \
and the Aux key (Alt or Option on other keyboards) types extra characters.",
        who, date
    )
}

/// The body of a Messages request for the conversation so far (which ends
/// with the person's turn).
pub fn request(model: &str, system: &str, turns: &[Turn]) -> Vec<u8> {
    // start at a turn of the person's, at most MAX_TURNS back
    let mut from = turns.len().saturating_sub(MAX_TURNS);
    while from < turns.len() && !turns[from].user {
        from += 1;
    }
    let mut msgs = Vec::new();
    for t in &turns[from..] {
        if t.text.trim().is_empty() {
            continue;
        }
        msgs.push(format!("{{\"role\":\"{}\",\"content\":{}}}", if t.user { "user" } else { "assistant" }, quote(&t.text)));
    }
    format!("{{\"model\":{},\"max_tokens\":{},\"system\":{},\"messages\":[{}]}}", quote(model), MAX_TOKENS, quote(system), msgs.join(",")).into_bytes()
}

/// Claude's answer: its text, and a note when it stopped early.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub text: String,
    pub note: Option<&'static str>,
}

/// Read a Messages response.
pub fn answer(status: u16, body: &[u8]) -> Result<Answer, String> {
    let text = String::from_utf8_lossy(body);
    let v = json::parse(&text);
    if status != 200 {
        return Err(error(status, v.as_ref()));
    }
    let v = v.ok_or_else(|| String::from("Claude's answer couldn't be read."))?;
    let stop = v.get("stop_reason").str().unwrap_or("");
    if stop == "refusal" {
        return Ok(Answer { text: String::new(), note: Some("Claude declined to answer that.") });
    }
    let mut out = String::new();
    for b in v.get("content").arr() {
        if b.get("type").str() == Some("text") {
            if let Some(t) = b.get("text").str() {
                out.push_str(t);
            }
        }
    }
    let note = match stop {
        "max_tokens" => Some("The answer was cut short: it reached the length limit."),
        _ => None,
    };
    Ok(Answer { text: plain(out.trim()), note })
}

/// A sentence for a failed request.
fn error(status: u16, v: Option<&Value>) -> String {
    let msg = v.and_then(|v| v.get("error").get("message").str()).unwrap_or("");
    let what = match status {
        400 => "Claude couldn't take that request",
        401 => return String::from("The API key wasn't accepted. Check it in Settings › Assistant."),
        403 => return String::from("This API key isn't allowed to use Claude. Check it in Settings › Assistant."),
        404 => return String::from("That Claude model isn't available to this key. Choose another in Settings › Assistant."),
        413 => return String::from("The conversation is too long. Start a new one."),
        429 => return String::from("Too many requests for now. Wait a moment and try again."),
        500..=599 => return String::from("Claude is busy right now. Try again in a moment."),
        _ => "The request failed",
    };
    if msg.is_empty() {
        format!("{} (HTTP {}).", what, status)
    } else {
        format!("{}: {}", what, msg)
    }
}

/// A model the key can use: (id, display name).
pub type Model = (String, String);

/// Read a Models response (newest first).
pub fn models(status: u16, body: &[u8]) -> Result<Vec<Model>, String> {
    let text = String::from_utf8_lossy(body);
    let v = json::parse(&text);
    if status != 200 {
        return Err(error(status, v.as_ref()));
    }
    let v = v.ok_or_else(|| String::from("The list of models couldn't be read."))?;
    let list: Vec<Model> = v
        .get("data")
        .arr()
        .iter()
        .filter_map(|m| {
            let id = m.get("id").str()?.to_string();
            let name = m.get("display_name").str().map(|s| s.to_string()).unwrap_or_else(|| id.clone());
            Some((id, name))
        })
        .collect();
    if list.is_empty() {
        Err(String::from("This API key has no Claude models."))
    } else {
        Ok(list)
    }
}

/// The model to use when none was chosen: the newest Opus, else the newest.
pub fn pick(models: &[Model]) -> Option<&Model> {
    models.iter().find(|m| m.0.contains("opus")).or(models.first())
}

/// Markdown left over in an answer, tidied for plain text: **bold**,
/// headings and code fences lose their marks.
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    for line in s.split('\n') {
        let l = line.trim_end();
        if l.trim_start().starts_with("```") {
            continue;
        }
        let l = if l.starts_with('#') { l.trim_start_matches('#').trim_start() } else { l };
        let l = l.replace("**", "").replace("__", "");
        let l = match l.strip_prefix("* ") {
            Some(rest) => format!("- {}", rest),
            None => l.clone(),
        };
        out.push_str(&l);
        out.push('\n');
    }
    out.pop();
    out
}
