//! The AI providers (#24): for each its id, base URL, models (one for
//! courses, one for the tutor) and wire format, and the one client for every
//! provider that speaks OpenAI's chat-completions format (OpenAI, Google's
//! OpenAI-compatible endpoint, DeepSeek, Mistral, xAI, OpenRouter and an
//! Ollama-compatible server). Anthropic's Messages API is in `ai.rs`. The
//! keys, labels and key formats are `secrets::Provider`'s.

use std::time::Duration;

use super::{agent, budget, call_header, send, Call, ChatMessage, Provider, Usage, CALL_HEADER};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    /// Gemini, through Google's OpenAI-compatible endpoint.
    Google,
    DeepSeek,
    Mistral,
    /// Grok.
    Xai,
    OpenRouter,
    /// Any server that speaks OpenAI's format at a base URL the owner gives
    /// (Ollama, LM Studio, llama.cpp's server); no key.
    Ollama,
}

/// How a provider is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    /// Anthropic's Messages API (`POST {base}/v1/messages`).
    Anthropic,
    /// OpenAI's chat completions (`POST {base}[/v1]/chat/completions`).
    /// `tokens` names the field for the most a reply may write (OpenAI's
    /// newer models refuse `max_tokens`; the others take only it), and
    /// `effort` says whether it takes `reasoning_effort`.
    Chat { tokens: &'static str, effort: bool },
}

/// The model Ollama runs when Manage Users names none.
pub const OLLAMA_DEFAULT_MODEL: &str = "llama3.1";

impl ProviderKind {
    pub const ALL: [ProviderKind; 8] = [
        ProviderKind::Anthropic,
        ProviderKind::OpenAi,
        ProviderKind::Google,
        ProviderKind::DeepSeek,
        ProviderKind::Mistral,
        ProviderKind::Xai,
        ProviderKind::OpenRouter,
        ProviderKind::Ollama,
    ];

    pub fn parse(s: &str) -> Option<ProviderKind> {
        ProviderKind::ALL.into_iter().find(|k| k.id() == s)
    }

    /// The short id (`anthropic`, `openai`, …), as forms name it.
    pub fn id(self) -> &'static str {
        self.secret().id()
    }

    /// The model a course is written with.
    pub fn default_model(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "claude-opus-5-5",
            ProviderKind::OpenAi => "gpt-5",
            ProviderKind::Google => "gemini-2.5-flash",
            ProviderKind::DeepSeek => "deepseek-chat",
            ProviderKind::Mistral => "mistral-large-latest",
            ProviderKind::Xai => "grok-4",
            ProviderKind::OpenRouter => "openrouter/auto",
            ProviderKind::Ollama => OLLAMA_DEFAULT_MODEL,
        }
    }

    /// The model the tutor talks with: a quicker one where the provider
    /// has it, else the course model.
    pub fn tutor_model(self) -> &'static str {
        match self {
            ProviderKind::Mistral => "mistral-medium-latest",
            ProviderKind::Xai => "grok-3-mini",
            other => other.default_model(),
        }
    }

    pub fn wire(self) -> Wire {
        match self {
            ProviderKind::Anthropic => Wire::Anthropic,
            ProviderKind::OpenAi | ProviderKind::Google => Wire::Chat { tokens: "max_completion_tokens", effort: true },
            ProviderKind::DeepSeek | ProviderKind::Mistral | ProviderKind::Xai | ProviderKind::OpenRouter | ProviderKind::Ollama => {
                Wire::Chat { tokens: "max_tokens", effort: false }
            }
        }
    }

    /// `SKRONGLE_<PROVIDER>_BASE_URL` (`SKRONGLE_OPENAI_BASE_URL`,
    /// `SKRONGLE_DEEPSEEK_BASE_URL`, …) points a provider elsewhere for every feature. Unset, dev mode points each at
    /// its simulated provider (`dev::provider_base_url`), else the public
    /// API. Ollama's is the one saved on Manage Users (`server_provider`);
    /// this is only its usual local address.
    pub fn default_base_url(self) -> String {
        let (var, public) = match self {
            ProviderKind::Anthropic => ("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
            ProviderKind::OpenAi => ("OPENAI_BASE_URL", "https://api.openai.com"),
            ProviderKind::Google => ("GOOGLE_BASE_URL", "https://generativelanguage.googleapis.com/v1beta/openai"),
            ProviderKind::DeepSeek => ("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
            ProviderKind::Mistral => ("MISTRAL_BASE_URL", "https://api.mistral.ai"),
            ProviderKind::Xai => ("XAI_BASE_URL", "https://api.x.ai"),
            ProviderKind::OpenRouter => ("OPENROUTER_BASE_URL", "https://openrouter.ai/api/v1"),
            // Its usual local address; the one used is Manage Users' (no variable).
            ProviderKind::Ollama => ("", "http://localhost:11434"),
        };
        let set = if var.is_empty() { None } else { crate::env(var) };
        set.or_else(|| crate::dev::provider_base_url(self)).unwrap_or_else(|| public.to_string())
    }

    /// Which of the server's keys it takes (Manage Users).
    pub fn secret(self) -> crate::secrets::Provider {
        use crate::secrets::Provider as P;
        match self {
            ProviderKind::Anthropic => P::Anthropic,
            ProviderKind::OpenAi => P::OpenAi,
            ProviderKind::Google => P::Google,
            ProviderKind::DeepSeek => P::DeepSeek,
            ProviderKind::Mistral => P::Mistral,
            ProviderKind::Xai => P::Xai,
            ProviderKind::OpenRouter => P::OpenRouter,
            ProviderKind::Ollama => P::Ollama,
        }
    }

    /// The provider as Manage Users stores its key (`secrets::Provider`).
    pub fn from_secret(p: crate::secrets::Provider) -> ProviderKind {
        ProviderKind::ALL.into_iter().find(|k| k.secret() == p).unwrap_or(ProviderKind::Anthropic)
    }

    /// A provider configured with its defaults (the course model) and this key.
    pub fn provider(self, api_key: String) -> Provider {
        Provider { kind: self, base_url: self.default_base_url(), api_key, model: self.default_model().to_string(), effort: None }
    }

    /// How a job's log names it.
    pub fn name(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::OpenAi => "an OpenAI-compatible API",
            ProviderKind::Google => "Google",
            ProviderKind::DeepSeek => "DeepSeek",
            ProviderKind::Mistral => "Mistral",
            ProviderKind::Xai => "xAI",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::Ollama => "an Ollama-compatible server",
        }
    }
}

pub(super) fn provider_name(kind: ProviderKind) -> &'static str {
    kind.name()
}

/// What a feature calls with: the server's provider for `kind`, from what
/// Manage Users stored: the key, or for Ollama the base URL (no key) and
/// the model its `settings` row names. The tutor (`for_tutor`) gets the
/// provider's tutor model. `None` when nothing is stored or it no longer
/// decrypts. Run inside `Db::call`.
pub fn server_provider(conn: &rusqlite::Connection, vault: &crate::secrets::Vault, kind: ProviderKind, for_tutor: bool) -> rusqlite::Result<Option<Provider>> {
    let Some(stored) = crate::secrets::get(conn, vault, kind.secret())? else { return Ok(None) };
    let mut p = kind.provider(stored);
    if kind == ProviderKind::Ollama {
        p.base_url = std::mem::take(&mut p.api_key);
        p.model = crate::settings::get(conn, crate::settings::OLLAMA_MODEL)?.unwrap_or_else(|| OLLAMA_DEFAULT_MODEL.to_string());
    } else if for_tutor {
        p.model = kind.tutor_model().to_string();
    }
    Ok(Some(p))
}

/// `usage` of a chat completion; `prompt_tokens` includes the cached ones.
pub(super) fn openai_usage(json: &serde_json::Value) -> Usage {
    let n = |k: &str| json.pointer(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let cached = n("/usage/prompt_tokens_details/cached_tokens");
    Usage { input: n("/usage/prompt_tokens").saturating_sub(cached), output: n("/usage/completion_tokens"), cached }
}

pub(super) fn openai_body(p: &Provider, system: &str, messages: &[ChatMessage], max_tokens: u32) -> serde_json::Value {
    let mut all = vec![serde_json::json!({ "role": "system", "content": system })];
    all.extend(messages.iter().map(|m| serde_json::json!({ "role": m.role, "content": m.content })));
    let (tokens, effort) = match p.kind.wire() {
        Wire::Chat { tokens, effort } => (tokens, effort),
        Wire::Anthropic => ("max_tokens", false),
    };
    let mut body = serde_json::json!({ "model": p.model, "messages": all });
    body[tokens] = max_tokens.into();
    if effort {
        budget::openai(&mut body, p);
    }
    body
}

/// A base URL that already ends in `/v1` (OpenRouter's) or `/openai`
/// (Google's) takes `/chat/completions`; the others (OpenAI's own,
/// DeepSeek's, Mistral's, xAI's, Ollama's) the `/v1` segment too.
pub(super) fn openai_url(p: &Provider) -> String {
    let base = p.base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.ends_with("/openai") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

/// The reply's text, or why there is none.
pub(super) fn openai_text(json: &serde_json::Value) -> Result<String, String> {
    let text = json
        .pointer("/choices/0/message/content")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or("the reply has no choices[0].message.content")?;
    if text.trim().is_empty() {
        return Err("the model returned no text".into());
    }
    Ok(text)
}

pub(super) fn call_openai(p: &Provider, call: &Call, system: &str, messages: &[ChatMessage], max_tokens: u32, timeout: Duration) -> Result<(String, Option<Usage>), String> {
    let body = openai_body(p, system, messages, max_tokens);
    let mut req = agent(timeout).post(&openai_url(p)).header("content-type", "application/json").header(CALL_HEADER, &call_header(call));
    // An Ollama-compatible server takes no key: no Authorization header.
    if !p.api_key.is_empty() {
        req = req.header("authorization", &format!("Bearer {}", p.api_key));
    }
    if p.kind == ProviderKind::OpenRouter {
        // OpenRouter shows the calling app by these; neither is required.
        req = req.header("x-title", crate::APP_NAME);
    }
    let json = send(req, &body, timeout)?;
    let text = openai_text(&json)?;
    Ok((text, Some(openai_usage(&json))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs() -> Vec<ChatMessage> {
        vec![ChatMessage { role: "user", content: "hi".into() }]
    }

    /// Every provider round-trips its id with `secrets::Provider`, and the
    /// two lists are in the same order.
    #[test]
    fn every_provider_has_a_key_row() {
        assert_eq!(ProviderKind::ALL.len(), crate::secrets::Provider::ALL.len());
        for (k, p) in ProviderKind::ALL.into_iter().zip(crate::secrets::Provider::ALL) {
            assert_eq!(k.secret(), p);
            assert_eq!(ProviderKind::from_secret(p), k);
            assert_eq!(ProviderKind::parse(k.id()), Some(k));
            assert!(!k.default_model().is_empty() && !k.tutor_model().is_empty());
        }
    }

    /// DeepSeek: OpenAI's format at api.deepseek.com/v1, `max_tokens`, no
    /// `reasoning_effort`, the key as a Bearer token; its reply maps as OpenAI's.
    #[test]
    fn deepseek_request_and_reply() {
        let p = Provider { effort: Some(super::super::Effort::Thorough), ..ProviderKind::DeepSeek.provider("sk-ds".into()) };
        assert_eq!(openai_url(&p), "https://api.deepseek.com/v1/chat/completions");
        let body = openai_body(&p, "sys", &msgs(), 500);
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["max_tokens"], 500);
        assert!(body.get("max_completion_tokens").is_none() && body.get("reasoning_effort").is_none());
        assert_eq!(body["messages"][0], serde_json::json!({"role": "system", "content": "sys"}));
        let reply = serde_json::json!({"choices": [{"message": {"role": "assistant", "content": "¡Hola!"}}], "usage": {"prompt_tokens": 12, "completion_tokens": 3, "prompt_tokens_details": {"cached_tokens": 2}}});
        assert_eq!(openai_text(&reply).unwrap(), "¡Hola!");
        assert_eq!(openai_usage(&reply), Usage { input: 10, output: 3, cached: 2 });
    }

    /// Mistral: api.mistral.ai/v1, `max_tokens`; a quicker tutor model; an empty reply is an error.
    #[test]
    fn mistral_request_and_reply() {
        let p = ProviderKind::Mistral.provider("k".into());
        assert_eq!(openai_url(&p), "https://api.mistral.ai/v1/chat/completions");
        let body = openai_body(&p, "sys", &msgs(), 300);
        assert_eq!((body["model"].as_str(), body["max_tokens"].as_u64()), (Some("mistral-large-latest"), Some(300)));
        assert_eq!(ProviderKind::Mistral.tutor_model(), "mistral-medium-latest");
        let empty = serde_json::json!({"choices": [{"message": {"content": "  "}}]});
        assert!(openai_text(&empty).unwrap_err().contains("no text"));
    }

    /// xAI: api.x.ai/v1, Grok models, `max_tokens`.
    #[test]
    fn xai_request_and_reply() {
        let p = ProviderKind::Xai.provider("xai-k".into());
        assert_eq!(openai_url(&p), "https://api.x.ai/v1/chat/completions");
        let body = openai_body(&p, "sys", &msgs(), 200);
        assert_eq!((body["model"].as_str(), body["max_tokens"].as_u64()), (Some("grok-4"), Some(200)));
        let reply = serde_json::json!({"choices": [{"message": {"content": "ok"}}], "usage": {"prompt_tokens": 4, "completion_tokens": 1}});
        assert_eq!((openai_text(&reply).unwrap().as_str(), openai_usage(&reply)), ("ok", Usage { input: 4, output: 1, cached: 0 }));
    }

    /// OpenRouter: the base URL already ends in /v1; its router model.
    #[test]
    fn openrouter_request_and_reply() {
        let p = ProviderKind::OpenRouter.provider("sk-or-k".into());
        assert_eq!(openai_url(&p), "https://openrouter.ai/api/v1/chat/completions");
        let body = openai_body(&p, "sys", &msgs(), 100);
        assert_eq!((body["model"].as_str(), body["max_tokens"].as_u64()), (Some("openrouter/auto"), Some(100)));
        assert!(openai_text(&serde_json::json!({"error": {"message": "x"}})).is_err());
    }

    /// Ollama: the owner's base URL and model from Manage Users, no key.
    #[test]
    fn ollama_takes_the_saved_url_and_model() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let vault = crate::secrets::Vault::from_master(&[5; 32]);
        db.with(|c| {
            assert!(server_provider(c, &vault, ProviderKind::Ollama, false)?.is_none());
            crate::secrets::save(c, &vault, crate::secrets::Provider::Ollama, "http://192.168.1.20:11434")?;
            let p = server_provider(c, &vault, ProviderKind::Ollama, true)?.unwrap();
            assert_eq!((p.base_url.as_str(), p.api_key.as_str(), p.model.as_str()), ("http://192.168.1.20:11434", "", OLLAMA_DEFAULT_MODEL));
            assert_eq!(openai_url(&p), "http://192.168.1.20:11434/v1/chat/completions");
            crate::settings::set(c, crate::settings::OLLAMA_MODEL, "qwen2.5:7b")?;
            let p = server_provider(c, &vault, ProviderKind::Ollama, false)?.unwrap();
            let body = openai_body(&p, "sys", &msgs(), 50);
            assert_eq!((body["model"].as_str(), body["max_tokens"].as_u64()), (Some("qwen2.5:7b"), Some(50)));
            // A keyed provider: the tutor gets its tutor model.
            crate::secrets::save(c, &vault, crate::secrets::Provider::Xai, "xai-abc")?;
            assert_eq!(server_provider(c, &vault, ProviderKind::Xai, true)?.unwrap().model, "grok-3-mini");
            assert_eq!(server_provider(c, &vault, ProviderKind::Xai, false)?.unwrap().model, "grok-4");
            Ok(())
        })
        .unwrap();
    }
}
