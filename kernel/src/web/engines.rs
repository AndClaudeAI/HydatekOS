//! Search engines the address bar can use: Hyda Search (on this computer)
//! and web search engines, chosen as the default or per search with a
//! shortcut like `!d nigerian jollof`.

use super::url::encode;
use alloc::string::String;

pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    /// the results address, with `{q}` for the query
    pub url: &'static str,
    /// typed as `!key` before or after a search
    pub key: &'static str,
    pub about: &'static str,
}

pub const HYDA: &str = "hyda";

pub const ENGINES: &[Engine] = &[
    Engine { id: HYDA, name: "Hyda Search", url: "hydatek://search?q={q}", key: "h", about: "HydatekOS's own: the pages you visit, sites you add and your files. Nothing leaves this computer." },
    Engine {
        id: "duckduckgo",
        name: "DuckDuckGo",
        url: "https://html.duckduckgo.com/html/?q={q}",
        key: "d",
        about: "Private web search. HydatekOS uses its plain HTML version, which works without JavaScript.",
    },
    Engine { id: "mojeek", name: "Mojeek", url: "https://www.mojeek.com/search?q={q}", key: "m", about: "An independent web search engine with its own index. Works without JavaScript." },
    Engine { id: "bing", name: "Bing", url: "https://www.bing.com/search?q={q}", key: "b", about: "Microsoft's web search." },
    Engine { id: "brave", name: "Brave Search", url: "https://search.brave.com/search?q={q}", key: "br", about: "Web search with its own index." },
    Engine {
        id: "google",
        name: "Google",
        url: "https://www.google.com/search?q={q}",
        key: "g",
        about: "Google's search now needs JavaScript, which HydatekOS's browser doesn't run yet, so its results may not show.",
    },
    Engine { id: "wikipedia", name: "Wikipedia", url: "https://en.wikipedia.org/w/index.php?search={q}", key: "w", about: "Search the encyclopaedia's articles." },
];

pub fn by_id(id: &str) -> &'static Engine {
    ENGINES.iter().find(|e| e.id == id).unwrap_or(&ENGINES[0])
}

pub fn search_url(e: &Engine, q: &str) -> String {
    e.url.replace("{q}", &encode(q))
}

/// A search with a `!key` shortcut in it: the engine and the rest.
pub fn shortcut(input: &str) -> Option<(&'static Engine, String)> {
    let mut engine = None;
    let mut rest = alloc::vec::Vec::new();
    for word in input.split_whitespace() {
        match word.strip_prefix('!').and_then(|k| ENGINES.iter().find(|e| e.key.eq_ignore_ascii_case(k))) {
            Some(e) if engine.is_none() => engine = Some(e),
            _ => rest.push(word),
        }
    }
    engine.map(|e| (e, rest.join(" ")))
}
