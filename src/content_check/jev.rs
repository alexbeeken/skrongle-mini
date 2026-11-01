//! A small client for Jev (TypeSafe System One), over the `ureq` client the
//! binary already has. One call asks typed yes/no questions ("noul") about a
//! piece of text and gets a probability for each: `POST <base>/v1/systemone`
//! with `{model, state, questions: {name: {type: "noul", instructions}}}`,
//! answered with `{answers: {name: {noul: p}}, usage: {input_tokens}}`. The
//! Node SDK in `e2e/node_modules/@typesafe-ai/sdk` speaks the same API.

use std::time::Duration;

/// Pinned, so a tuned threshold doesn't drift when the `jev-latest` alias moves.
pub const MODEL: &str = "jev-1.13.0";
/// Dollars per input token (output is free), from docs.typesafe.ai/models.
pub const PRICE_PER_TOKEN: f64 = 0.042 / 1_000_000.0;
/// The public API.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
/// One call answers in about a second; a stuck one shouldn't hold the jobs thread.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The largest reply read (a few hundred bytes are expected).
const MAX_REPLY_BYTES: u64 = 64 * 1024;

/// Where Jev is and the key to it.
#[derive(Clone)]
pub struct Jev {
    pub base_url: String,
    key: String,
    pub model: String,
}

impl std::fmt::Debug for Jev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jev").field("base_url", &self.base_url).field("model", &self.model).finish_non_exhaustive()
    }
}

/// The answers of one call, by question name, and what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Answers {
    pub probabilities: Vec<(String, f64)>,
    pub input_tokens: i64,
}

impl Answers {
    pub fn get(&self, name: &str) -> f64 {
        self.probabilities.iter().find(|(n, _)| n == name).map_or(0.0, |(_, p)| *p)
    }

    pub fn cost_usd(&self) -> f64 {
        self.input_tokens as f64 * PRICE_PER_TOKEN
    }
}

impl Jev {
    pub fn new(base_url: &str, key: &str) -> Jev {
        Jev { base_url: base_url.trim_end_matches('/').to_string(), key: key.to_string(), model: MODEL.to_string() }
    }

    /// Ask every `(name, statement)` of `questions` about `state` in one call.
    pub fn ask(&self, state: &str, questions: &[(&str, &str)]) -> Result<Answers, String> {
        let qs: serde_json::Map<String, serde_json::Value> =
            questions.iter().map(|(n, s)| (n.to_string(), serde_json::json!({"type": "noul", "instructions": s}))).collect();
        let body = serde_json::json!({"model": self.model, "state": state, "questions": qs});
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).max_redirects(0).http_status_as_error(false).build().into();
        let mut resp = agent
            .post(&format!("{}/v1/systemone", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.key))
            .header("Accept", "application/json")
            .send_json(&body)
            .map_err(|e| format!("Jev: request failed: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().with_config().limit(MAX_REPLY_BYTES).read_to_string().map_err(|e| format!("Jev: could not read the reply: {e}"))?;
        if status >= 300 {
            return Err(format!("Jev: HTTP {status}: {}", text.chars().take(200).collect::<String>()));
        }
        parse(&text, questions)
    }
}

/// Read a reply: every question must have its probability.
pub fn parse(text: &str, questions: &[(&str, &str)]) -> Result<Answers, String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|_| format!("Jev: the reply is not JSON: {}", text.chars().take(200).collect::<String>()))?;
    let mut probabilities = Vec::new();
    for (name, _) in questions {
        let p = json
            .pointer(&format!("/answers/{name}/noul"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("Jev: no answer to {name:?} in the reply"))?;
        probabilities.push((name.to_string(), p.clamp(0.0, 1.0)));
    }
    let input_tokens = json.pointer("/usage/input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(Answers { probabilities, input_tokens })
}

// ---------- the dev server's stand-in ----------

/// The key the dev server's stand-in (`/dev/ai/jev`) takes.
pub const STAND_IN_KEY: &str = "dev-jev";
/// Where the stand-in reads its scores.
pub const STAND_IN_FIXTURE: &str = "data/test-fixtures/jev/content-check.json";

/// The stand-in's reply to `req`: each question gets the score of the first
/// rule in `fixture` whose `contains` is in the text (any case) and that
/// names the question, else `default`. Tokens are the text's length over four.
pub fn stand_in(fixture: &serde_json::Value, req: &serde_json::Value) -> serde_json::Value {
    let state = req.get("state").and_then(|s| s.as_str()).unwrap_or("");
    let lower = state.to_lowercase();
    let default = fixture.get("default").and_then(|d| d.as_f64()).unwrap_or(0.02);
    let rules = fixture.get("rules").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let mut answers = serde_json::Map::new();
    for name in req.get("questions").and_then(|q| q.as_object()).map(|q| q.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let p = rules
            .iter()
            .filter(|r| r.get("contains").and_then(|c| c.as_str()).is_some_and(|c| lower.contains(&c.to_lowercase())))
            .find_map(|r| r.get(&name).and_then(|v| v.as_f64()))
            .unwrap_or(default);
        answers.insert(name, serde_json::json!({"type": "noul", "noul": p}));
    }
    serde_json::json!({"model": req.get("model"), "answers": answers, "usage": {"input_tokens": state.len() / 4, "output_tokens": 10}})
}
