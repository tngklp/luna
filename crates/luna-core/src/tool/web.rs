//! Web tools: fetch a page, search the web, read a PDF.
//!
//! Fetching is deliberately conservative: a size cap, a timeout, and HTML
//! reduced to text so a page costs tokens in proportion to its prose rather
//! than its markup.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{optional_str, optional_usize, required_str, Tool, ToolClass, ToolContext, ToolOutput};
use crate::error::{LunaError, Result};

/// Pages larger than this are truncated before they reach the model.
const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Characters of extracted text kept.
const MAX_TEXT: usize = 40_000;

fn client() -> reqwest::Client {
    static CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(45))
            .user_agent("luna/0.1 (+https://github.com/tngklp/luna)")
            .build()
            .unwrap_or_default()
    });
    CLIENT.clone()
}

/// Reduce HTML to readable text.
///
/// A full parser would be a large dependency for what is ultimately a
/// token-saving measure, so this drops script/style blocks, turns block-level
/// tags into newlines, strips the rest, and decodes the handful of entities that
/// actually appear in prose.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut skipping: Option<&str> = None;

    while !rest.is_empty() {
        if let Some(tag) = skipping {
            match rest.find(tag) {
                Some(index) => {
                    rest = &rest[index + tag.len()..];
                    skipping = None;
                }
                None => break,
            }
            continue;
        }

        let Some(open) = rest.find('<') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let Some(close) = tail.find('>') else {
            break;
        };
        let tag = &tail[1..close];
        rest = &tail[close + 1..];

        let name: String = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        match name.as_str() {
            "script" => skipping = Some("</script>"),
            "style" => skipping = Some("</style>"),
            "noscript" => skipping = Some("</noscript>"),
            "br" => out.push('\n'),
            "p" | "div" | "section" | "article" | "li" | "tr" | "h1" | "h2" | "h3" | "h4"
            | "h5" | "h6" | "blockquote" | "pre" => out.push('\n'),
            "td" | "th" => out.push(' '),
            _ => {}
        }
    }

    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…");

    // Collapse runs of blank lines and trailing spaces.
    let mut cleaned = String::with_capacity(decoded.len());
    let mut blank = 0;
    for line in decoded.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        cleaned.push_str(trimmed);
        cleaned.push('\n');
    }
    cleaned.trim().to_string()
}

/// Truncate to a character budget, saying so.
fn cap(text: String, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}\n\n… truncated at {limit} characters")
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its text. HTML is reduced to readable prose, so \
         the result costs tokens in proportion to the content, not the markup."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Absolute http(s) URL" },
                "raw": { "type": "boolean", "description": "Return the body as-is instead of extracting text" }
            },
            "required": ["url"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Network
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let url = required_str(&args, "url")?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutput::error("only http and https URLs can be fetched"));
        }
        let raw = args.get("raw").and_then(|value| value.as_bool()).unwrap_or(false);

        let response = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => return Err(LunaError::Cancelled),
            result = client().get(&url).send() => result,
        }
        .map_err(|err| LunaError::tool(format!("could not fetch {url}: {err}")))?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!("{url} returned {status}")));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = response
            .bytes()
            .await
            .map_err(|err| LunaError::tool(format!("could not read {url}: {err}")))?;
        if bytes.len() > MAX_BYTES {
            return Ok(ToolOutput::error(format!(
                "{url} is {} bytes; the limit is {MAX_BYTES}",
                bytes.len()
            )));
        }

        let body = String::from_utf8_lossy(&bytes).to_string();
        let is_html = content_type.contains("html") || body.trim_start().starts_with('<');
        let text = if raw || !is_html { body } else { html_to_text(&body) };

        if text.trim().is_empty() {
            return Ok(ToolOutput::ok(format!("{url} returned no readable text")));
        }
        Ok(ToolOutput::ok(cap(text, MAX_TEXT)))
    }
}

/// Where a search goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBackend {
    /// DuckDuckGo's HTML endpoint. No key, no tracking, rate-limited.
    DuckDuckGo,
    /// Brave Search API. Needs `BRAVE_API_KEY`.
    Brave,
    /// Tavily. Needs `TAVILY_API_KEY`.
    Tavily,
}

impl SearchBackend {
    pub fn from_config(value: &str) -> SearchBackend {
        match value {
            "brave" => SearchBackend::Brave,
            "tavily" => SearchBackend::Tavily,
            _ => SearchBackend::DuckDuckGo,
        }
    }
}

pub struct WebSearchTool {
    backend: SearchBackend,
}

impl WebSearchTool {
    pub fn new(backend: SearchBackend) -> Self {
        WebSearchTool { backend }
    }

    /// The backend to use when config says nothing: whichever has a key.
    pub fn auto() -> Self {
        let backend = if std::env::var("BRAVE_API_KEY").is_ok() {
            SearchBackend::Brave
        } else if std::env::var("TAVILY_API_KEY").is_ok() {
            SearchBackend::Tavily
        } else {
            SearchBackend::DuckDuckGo
        };
        WebSearchTool { backend }
    }
}

/// One search result.
#[derive(Debug, Clone)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl Hit {
    fn render(&self, index: usize) -> String {
        if self.snippet.is_empty() {
            format!("{index}. {}\n   {}", self.title, self.url)
        } else {
            format!("{index}. {}\n   {}\n   {}", self.title, self.url, self.snippet)
        }
    }
}

/// Pull results out of DuckDuckGo's HTML page.
pub fn parse_duckduckgo(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    // Each result is an anchor with class result__a, followed by a snippet.
    for chunk in html.split("result__a").skip(1) {
        let Some(href_start) = chunk.find("href=\"") else { continue };
        let after = &chunk[href_start + 6..];
        let Some(href_end) = after.find('"') else { continue };
        let url = decode_ddg_url(&after[..href_end]);

        let Some(text_start) = after.find('>') else { continue };
        let after_text = &after[text_start + 1..];
        let Some(text_end) = after_text.find("</a>") else { continue };
        let title = html_to_text(&after_text[..text_end]);

        let snippet = chunk
            .find("result__snippet")
            .and_then(|index| {
                let tail = &chunk[index..];
                let open = tail.find('>')?;
                let body = &tail[open + 1..];
                let close = body.find("</a>").or_else(|| body.find("</div>"))?;
                Some(html_to_text(&body[..close]))
            })
            .unwrap_or_default();

        if !url.is_empty() && !title.is_empty() {
            hits.push(Hit { title, url, snippet });
        }
        if hits.len() >= 10 {
            break;
        }
    }
    hits
}

/// DuckDuckGo wraps results in a redirect; the real URL is in the `uddg` param.
fn decode_ddg_url(href: &str) -> String {
    let Some(index) = href.find("uddg=") else {
        return href.to_string();
    };
    let rest = &href[index + 5..];
    let end = rest.find('&').unwrap_or(rest.len());
    percent_decode(&rest[..end])
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web and return titles, URLs and snippets. Use `web_fetch` to \
         read a result in full."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "description": "Results to return, default 5" }
            },
            "required": ["query"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Network
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let query = required_str(&args, "query")?;
        let limit = optional_usize(&args, "limit").unwrap_or(5).clamp(1, 10);

        let hits = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => return Err(LunaError::Cancelled),
            result = self.search(&query) => result?,
        };

        if hits.is_empty() {
            return Ok(ToolOutput::ok(format!("no results for `{query}`")));
        }
        let body = hits
            .iter()
            .take(limit)
            .enumerate()
            .map(|(index, hit)| hit.render(index + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(ToolOutput::ok(body))
    }
}

impl WebSearchTool {
    async fn search(&self, query: &str) -> Result<Vec<Hit>> {
        match self.backend {
            SearchBackend::DuckDuckGo => {
                let response = client()
                    .get("https://html.duckduckgo.com/html/")
                    .query(&[("q", query)])
                    .send()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                let body = response
                    .text()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                Ok(parse_duckduckgo(&body))
            }
            SearchBackend::Brave => {
                let key = std::env::var("BRAVE_API_KEY")
                    .map_err(|_| LunaError::tool("BRAVE_API_KEY is not set"))?;
                let response = client()
                    .get("https://api.search.brave.com/res/v1/web/search")
                    .header("X-Subscription-Token", key)
                    .query(&[("q", query), ("count", "10")])
                    .send()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                let value: Value = response
                    .json()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                Ok(value
                    .get("web")
                    .and_then(|web| web.get("results"))
                    .and_then(|results| results.as_array())
                    .map(|results| {
                        results
                            .iter()
                            .filter_map(|entry| {
                                Some(Hit {
                                    title: entry.get("title")?.as_str()?.to_string(),
                                    url: entry.get("url")?.as_str()?.to_string(),
                                    snippet: entry
                                        .get("description")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default())
            }
            SearchBackend::Tavily => {
                let key = std::env::var("TAVILY_API_KEY")
                    .map_err(|_| LunaError::tool("TAVILY_API_KEY is not set"))?;
                let response = client()
                    .post("https://api.tavily.com/search")
                    .json(&json!({ "api_key": key, "query": query, "max_results": 10 }))
                    .send()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                let value: Value = response
                    .json()
                    .await
                    .map_err(|err| LunaError::tool(format!("search failed: {err}")))?;
                Ok(value
                    .get("results")
                    .and_then(|results| results.as_array())
                    .map(|results| {
                        results
                            .iter()
                            .filter_map(|entry| {
                                Some(Hit {
                                    title: entry.get("title")?.as_str()?.to_string(),
                                    url: entry.get("url")?.as_str()?.to_string(),
                                    snippet: entry
                                        .get("content")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default())
            }
        }
    }
}

pub struct ReadPdfTool;

#[async_trait]
impl Tool for ReadPdfTool {
    fn name(&self) -> &str {
        "read_pdf"
    }

    fn description(&self) -> &str {
        "Extract the text of a PDF, page by page. Use `pages` to read a range of a \
         long document."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, or an http(s) URL" },
                "pages": { "type": "string", "description": "Page range, e.g. `1-5` or `3`" }
            },
            "required": ["path"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = required_str(&args, "path")?;
        let bytes = if path.starts_with("http://") || path.starts_with("https://") {
            let response = tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => return Err(LunaError::Cancelled),
                result = client().get(&path).send() => result,
            }
            .map_err(|err| LunaError::tool(format!("could not fetch {path}: {err}")))?;
            response
                .bytes()
                .await
                .map_err(|err| LunaError::tool(format!("could not read {path}: {err}")))?
                .to_vec()
        } else {
            let resolved = ctx.resolve(&path)?;
            std::fs::read(&resolved).map_err(|source| LunaError::Io { path: resolved, source })?
        };

        let text = pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|err| LunaError::tool(format!("could not read the PDF: {err}")))?;

        let selected = match optional_str(&args, "pages") {
            Some(range) => select_pages(&text, &range),
            None => text,
        };
        if selected.trim().is_empty() {
            return Ok(ToolOutput::ok("the PDF contains no extractable text"));
        }
        Ok(ToolOutput::ok(cap(selected, MAX_TEXT)))
    }
}

/// Keep only the requested pages.
///
/// `pdf-extract` separates pages with a form feed, which is the only reliable
/// marker available without a full PDF parser.
fn select_pages(text: &str, range: &str) -> String {
    let pages: Vec<&str> = text.split('\u{c}').collect();
    let (start, end) = match range.split_once('-') {
        Some((a, b)) => (
            a.trim().parse::<usize>().unwrap_or(1),
            b.trim().parse::<usize>().unwrap_or(usize::MAX),
        ),
        None => {
            let page = range.trim().parse::<usize>().unwrap_or(1);
            (page, page)
        }
    };
    pages
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            let page = index + 1;
            page >= start && page <= end
        })
        .map(|(_, body)| *body)
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_reduced_to_prose() {
        let html = "<html><head><style>a{}</style></head><body><h1>Title</h1><p>Hello <b>world</b>.</p><script>x()</script></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world."));
        assert!(!text.contains("a{}"));
        assert!(!text.contains("x()"));
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(html_to_text("<p>a &amp; b &lt;c&gt;</p>"), "a & b <c>");
    }

    #[test]
    fn blank_lines_are_collapsed() {
        let text = html_to_text("<p>one</p><p></p><p></p><p>two</p>");
        assert!(!text.contains("\n\n\n"));
    }

    #[test]
    fn duckduckgo_results_are_parsed() {
        let html = r#"
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&amp;rut=x">First</a>
            <a class="result__snippet">A snippet</a>
        "#;
        let hits = parse_duckduckgo(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://example.com/a");
        assert_eq!(hits[0].title, "First");
        assert_eq!(hits[0].snippet, "A snippet");
    }

    #[test]
    fn percent_decoding_handles_utf8() {
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn page_ranges_select_the_right_pages() {
        let text = "one\u{c}two\u{c}three";
        assert_eq!(select_pages(text, "2"), "two");
        assert_eq!(select_pages(text, "2-3"), "two\n\nthree");
        assert_eq!(select_pages(text, "1-1"), "one");
    }

    #[test]
    fn long_text_is_capped() {
        let capped = cap("x".repeat(100), 10);
        assert!(capped.contains("truncated"));
        assert!(capped.chars().count() < 100);
    }

    #[test]
    fn the_backend_defaults_to_duckduckgo() {
        assert_eq!(SearchBackend::from_config("nonsense"), SearchBackend::DuckDuckGo);
        assert_eq!(SearchBackend::from_config("brave"), SearchBackend::Brave);
    }
}
