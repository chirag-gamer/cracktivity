//! Chat engines (spec §6.2/§6.3): one streaming interface, three wire
//! implementations — llama-server (OpenAI-compatible, loopback), OpenAI/xAI
//! BYO keys (same wire), and Anthropic's native `/v1/messages` SSE (preferred
//! for Anthropic keys — their OpenAI-compat endpoint is not
//! production-grade). Fallback chain per FR-24: the chosen engine fails →
//! local model (if enabled) with a visible notice; never silent cloud
//! fallback.

use std::io::{BufRead, BufReader};

use secrecy::ExposeSecret;

fn log_host(base_or_url: &str) {
    crate::platform::netlog::record_url(base_or_url);
}

/// Anthropic native endpoint (V-5.2 audit pin: host is in the netlog
/// inventory; the test asserts it).
pub(crate) const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, Clone)]
pub struct Msg {
    /// "system" | "user" | "assistant"
    pub role: &'static str,
    pub content: String,
}

impl Msg {
    pub fn system(content: impl Into<String>) -> Self {
        Msg {
            role: "system",
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Msg {
            role: "user",
            content: content.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ChatError(pub String);

/// A streaming chat engine: calls `on_delta` per text fragment and returns
/// the full text. Blocking (drivers run off the async runtime).
pub trait ChatEngine {
    fn stream_chat(
        &self,
        msgs: &[Msg],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, ChatError>;
}

/// Which BYO/local engine to use for a task (spec §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Local,
    OpenAi,
    Anthropic,
    Xai,
}

impl EngineKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" => Some(EngineKind::Local),
            "openai" => Some(EngineKind::OpenAi),
            "anthropic" => Some(EngineKind::Anthropic),
            "xai" => Some(EngineKind::Xai),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Local => "local",
            EngineKind::OpenAi => "openai",
            EngineKind::Anthropic => "anthropic",
            EngineKind::Xai => "xai",
        }
    }

    /// Default model id per engine (the user can override in Settings).
    pub fn default_model(self) -> &'static str {
        match self {
            EngineKind::Local => "local",
            EngineKind::OpenAi => "gpt-4o-mini",
            EngineKind::Anthropic => "claude-3-5-haiku-latest",
            EngineKind::Xai => "grok-2-latest",
        }
    }
}

/// OpenAI-compatible chat completions (llama-server, api.openai.com,
/// api.x.ai). `api_key` is empty for the local sidecar... no: llama-server
/// runs with a per-run key, so callers always supply one.
#[derive(Clone)]
pub struct OpenAiCompat {
    pub base_url: String,
    pub api_key: secrecy::SecretString,
    pub model: String,
}

impl OpenAiCompat {
    pub fn openai(key: &str, model: &str) -> Self {
        OpenAiCompat {
            base_url: "https://api.openai.com".into(),
            api_key: secrecy::SecretString::from(key.to_string()),
            model: model.to_string(),
        }
    }

    pub fn xai(key: &str, model: &str) -> Self {
        OpenAiCompat {
            base_url: "https://api.x.ai".into(),
            api_key: secrecy::SecretString::from(key.to_string()),
            model: model.to_string(),
        }
    }

    pub fn local(port: u16, key: &str) -> Self {
        OpenAiCompat {
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: secrecy::SecretString::from(key.to_string()),
            model: "local".into(),
        }
    }

    fn request_body(&self, msgs: &[Msg]) -> String {
        let messages: Vec<serde_json::Value> = msgs
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();
        serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        })
        .to_string()
    }
}

impl ChatEngine for OpenAiCompat {
    fn stream_chat(
        &self,
        msgs: &[Msg],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, ChatError> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(300)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        log_host(&self.base_url);
        let response = agent
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .header(
                "Authorization",
                &format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .send(self.request_body(msgs))
            .map_err(|e| match e {
                ureq::Error::StatusCode(code) => ChatError(format!("API error {code}")),
                other => ChatError(format!("connection failed: {other}")),
            })?;
        let reader = response.into_body().into_reader();
        parse_openai_sse(reader, on_delta)
    }
}

/// OpenAI chat-completions SSE: `data: {…choices[0].delta.content…}` lines,
/// terminated by `data: [DONE]`. Extracted for the L7 wire-contract tests.
pub fn parse_openai_sse(
    reader: impl std::io::Read,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, ChatError> {
    let mut full = String::new();
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| ChatError(format!("stream read: {e}")))?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data.trim() == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
            if !delta.is_empty() {
                on_delta(delta);
                full.push_str(delta);
            }
        }
    }
    Ok(full)
}

/// Anthropic native `/v1/messages` SSE (spec §6.3: preferred for Anthropic
/// keys). Events carry `content_block_delta` with `delta.text`.
#[derive(Clone)]
pub struct Anthropic {
    pub api_key: secrecy::SecretString,
    pub model: String,
    /// anthropic-version date (the current stable header value).
    pub version: &'static str,
}

impl Anthropic {
    pub fn new(key: &str, model: &str) -> Self {
        Anthropic {
            api_key: secrecy::SecretString::from(key.to_string()),
            model: model.to_string(),
            version: "2023-06-01",
        }
    }

    fn request_body(&self, msgs: &[Msg]) -> String {
        // The system prompt moves to its own field; messages alternate user/
        // assistant only.
        let system: Vec<String> = msgs
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect();
        let messages: Vec<serde_json::Value> = msgs
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();
        serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "stream": true,
            "system": system.join("\n\n"),
            "messages": messages,
        })
        .to_string()
    }
}

impl ChatEngine for Anthropic {
    fn stream_chat(
        &self,
        msgs: &[Msg],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, ChatError> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(300)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        log_host(ANTHROPIC_URL);
        let response = agent
            .post(ANTHROPIC_URL)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", self.version)
            .header("Content-Type", "application/json")
            .send(self.request_body(msgs))
            .map_err(|e| match e {
                ureq::Error::StatusCode(code) => ChatError(format!("API error {code}")),
                other => ChatError(format!("connection failed: {other}")),
            })?;
        let reader = response.into_body().into_reader();
        parse_anthropic_sse(reader, on_delta)
    }
}

/// Anthropic SSE: `data: {…}` lines whose JSON `type` selects the event;
/// text lives in `content_block_delta` → `delta.text`. Extracted for the L7
/// wire-contract tests.
pub fn parse_anthropic_sse(
    reader: impl std::io::Read,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, ChatError> {
    let mut full = String::new();
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| ChatError(format!("stream read: {e}")))?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if v["type"] == "content_block_delta" {
            if let Some(text) = v["delta"]["text"].as_str() {
                if !text.is_empty() {
                    on_delta(text);
                    full.push_str(text);
                }
            }
        } else if v["type"] == "message_stop" {
            break;
        }
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V-5.2 audit pin: BYO engine endpoints stay on the inventoried hosts.
    #[test]
    fn byo_endpoints_stay_on_inventored_hosts() {
        let openai = OpenAiCompat::openai("k", "m");
        assert!(openai
            .base_url
            .contains(crate::platform::netlog::HOST_OPENAI));
        let xai = OpenAiCompat::xai("k", "m");
        assert!(xai.base_url.contains(crate::platform::netlog::HOST_XAI));
        assert!(ANTHROPIC_URL.contains(crate::platform::netlog::HOST_ANTHROPIC));
    }

    /// L7 wire-contract: OpenAI chunk shape, delta extraction, [DONE] end.
    #[test]
    fn openai_sse_contract() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                      data: [DONE]\n\n";
        let mut pieces = Vec::new();
        let full = parse_openai_sse(&stream[..], &mut |d| pieces.push(d.to_string())).unwrap();
        assert_eq!(full, "Hello");
        assert_eq!(pieces, vec!["Hel", "lo"]);
    }

    /// L7 wire-contract: Anthropic event flow — only text deltas surface.
    #[test]
    fn anthropic_sse_contract() {
        let stream = b"event: message_start\n\
                      data: {\"type\":\"message_start\"}\n\n\
                      event: content_block_delta\n\
                      data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi \"}}\n\n\
                      data: {\"type\":\"ping\"}\n\n\
                      data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n\
                      data: {\"type\":\"message_stop\"}\n\n";
        let mut pieces = Vec::new();
        let full = parse_anthropic_sse(&stream[..], &mut |d| pieces.push(d.to_string())).unwrap();
        assert_eq!(full, "Hi there");
        assert_eq!(pieces, vec!["Hi ", "there"]);
    }

    /// Request bodies: Anthropic moves the system prompt out of messages and
    /// requires max_tokens (its API rejects otherwise).
    #[test]
    fn anthropic_request_shape() {
        let e = Anthropic::new("k", "claude-3-5-haiku-latest");
        let body: serde_json::Value =
            serde_json::from_str(&e.request_body(&[Msg::system("be brief"), Msg::user("hello")]))
                .unwrap();
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    /// OpenAI request shape keeps the system message inline.
    #[test]
    fn openai_request_shape() {
        let e = OpenAiCompat::openai("k", "gpt-4o-mini");
        let body: serde_json::Value =
            serde_json::from_str(&e.request_body(&[Msg::system("be brief"), Msg::user("hello")]))
                .unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    }
}
