//! Web tool: HTTP fetch with size/time limits and HTML-to-text extraction.

use super::registry::{Tool, ToolOutcome};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
const MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct WebFetch {
    http: reqwest::Client,
}

impl WebFetch {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(MAX_TIMEOUT)
                .user_agent("luna-agent/0.1")
                .build()
                .map_err(|e| Error::Tool(format!("web_fetch client: {e}")))?,
        })
    }
}

impl Default for WebFetch {
    fn default() -> Self {
        Self::new().expect("web_fetch client")
    }
}

/// Very small HTML → text extraction: strips script/style, tags, collapses
/// whitespace. Good enough for agent consumption; no full DOM parsing.
pub fn html_to_text(html: &str) -> String {
    // Remove script/style blocks with their content (no backreferences in
    // the regex crate, so match each tag explicitly).
    let script_re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("regex");
    let style_re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("regex");
    let tag_re = regex::Regex::new(r"(?s)<[^>]+>").expect("regex");
    let ws_re = regex::Regex::new(r"\n\s*\n+").expect("regex");

    let no_scripts = script_re.replace_all(html, "");
    let no_styles = style_re.replace_all(&no_scripts, "");
    // Keep alt text from images, then drop tags.
    let alt_re = regex::Regex::new(r#"(?i)<img[^>]*alt="([^"]*)"[^>]*>"#).expect("regex");
    let with_alts = alt_re.replace_all(&no_styles, " [image: $1] ");
    let no_tags = tag_re.replace_all(&with_alts, " ");
    // Decode the handful of entities that matter most.
    let decoded = no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    let collapsed = ws_re.replace_all(&decoded, "\n");
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL over HTTP(S) and return the response body as text (HTML is converted to plain text)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch (http/https)" }
            },
            "required": ["url"]
        })
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, cancel: CancellationToken) -> Result<ToolOutcome> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| Error::Tool("web_fetch: missing `url`".into()))?;

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutcome::err("web_fetch: only http/https URLs are supported"));
        }
        // Block obvious localhost/internal targets (SSRF guard).
        if let Ok(parsed) = url::Url::parse(url)
            && let Some(host) = parsed.host_str() {
                let lower = host.to_lowercase();
                if lower == "localhost"
                    || lower.ends_with(".local")
                    || lower == "0.0.0.0"
                    || lower.starts_with("127.")
                    || lower.starts_with("10.")
                    || lower.starts_with("192.168.")
                    || lower.starts_with("169.254.")
                    || lower.starts_with("[::1")
                {
                    return Ok(ToolOutcome::err(format!(
                        "web_fetch: refusing to fetch internal host `{host}`"
                    )));
                }
            }

        let resp = tokio::select! {
            r = self.http.get(url).send() => r,
            _ = cancel.cancelled() => return Err(Error::Cancelled),
        }
        .map_err(|e| Error::Tool(format!("web_fetch {url}: {e}")))?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Read the body with a hard size cap.
        let body = tokio::select! {
            b = resp.bytes() => b,
            _ = cancel.cancelled() => return Err(Error::Cancelled),
        }
        .map_err(|e| Error::Tool(format!("web_fetch {url} body: {e}")))?;

        let truncated = body.len() > MAX_BODY_BYTES;
        let bytes = if truncated { &body[..MAX_BODY_BYTES] } else { &body[..] };

        let text = if content_type.starts_with("text/html") {
            html_to_text(&String::from_utf8_lossy(bytes))
        } else {
            String::from_utf8_lossy(bytes).to_string()
        };

        let mut out = format!("[{status}] {url}\n");
        if truncated {
            out.push_str(&format!("[truncated at {MAX_BODY_BYTES} bytes]\n"));
        }
        out.push_str(&text);
        Ok(ToolOutcome { content: out, is_error: !status.is_success() })
    }
}

/// Register built-ins plus the web tool.
pub fn register_with_web(registry: &mut super::registry::ToolRegistry) -> Result<()> {
    super::builtin::register_all(registry);
    registry.register(std::sync::Arc::new(WebFetch::new()?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_strips_tags_and_scripts() {
        let html = r#"<html><head><style>body{color:red}</style></head>
            <body><h1>Hello</h1><script>alert("x")</script><p>World &amp; more</p>
            <img src="x.png" alt="a cat"></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World & more"));
        assert!(text.contains("[image: a cat]"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }

    #[tokio::test]
    async fn rejects_non_http_schemes() {
        let t = WebFetch::new().unwrap();
        let out = t
            .execute(json!({"url": "file:///etc/passwd"}), CancellationToken::new())
            .await
            .unwrap();
        assert!(out.is_error);
    }

    #[tokio::test]
    async fn blocks_localhost() {
        let t = WebFetch::new().unwrap();
        let out = t
            .execute(json!({"url": "http://localhost:8080/"}), CancellationToken::new())
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("internal host"));
    }
}
