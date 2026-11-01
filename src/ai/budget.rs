//! How much a course job may spend: the effort the learner picks for it
//! (Fast, Balanced, Thorough; #918), what each request may write, and the
//! tokens a job is expected to use, which the server's key reserves up front.

use super::{Provider, FORMAT_DOC};

/// The most tokens one generation request may write at Fast and Balanced.
pub(super) const GENERATE_MAX_TOKENS: u32 = 16000;

/// Input tokens of a job request, for its up-front estimate: the course
/// format reference and the learner's material go with every request.
pub const JOB_PROMPT_TOKENS: u64 = 2_000;
/// Output tokens of an outline or revision plan, and of one lesson.
pub const OUTLINE_OUTPUT_TOKENS: u64 = 3_000;
pub const LESSON_OUTPUT_TOKENS: u64 = 6_000;

/// The tokens a job is expected to use: `lessons` lesson requests, plus an
/// outline or plan request when `plan`. Retries are not counted; a job that
/// runs past its estimate goes on while the learner has allowance left.
/// Thorough thinks longer: [`Effort::output_tokens`] scales the output.
pub fn job_tokens(lessons: usize, plan: bool, notes_chars: usize) -> (u64, u64) {
    let per_request = (FORMAT_DOC.len() + notes_chars).div_ceil(4) as u64 + JOB_PROMPT_TOKENS;
    let requests = lessons as u64 + u64::from(plan);
    (requests * per_request, lessons as u64 * LESSON_OUTPUT_TOKENS + u64::from(plan) * OUTLINE_OUTPUT_TOKENS)
}

/// How hard the model works on a course: the same model at every level,
/// sent as `output_config.effort` to Anthropic and `reasoning_effort` to
/// OpenAI and Gemini. `users.ai_effort` keeps the account's last choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effort {
    Fast,
    #[default]
    Balanced,
    Thorough,
}

impl Effort {
    pub const ALL: [Effort; 3] = [Effort::Fast, Effort::Balanced, Effort::Thorough];

    /// The level as the form and the database name it, which is also what
    /// every provider takes: `low`, `medium`, `high`.
    pub fn id(self) -> &'static str {
        match self {
            Effort::Fast => "low",
            Effort::Balanced => "medium",
            Effort::Thorough => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.id() == s.trim())
    }

    /// Its i18n key part: `courses.generate.effort.<key>`.
    pub fn key(self) -> &'static str {
        match self {
            Effort::Fast => "fast",
            Effort::Balanced => "balanced",
            Effort::Thorough => "thorough",
        }
    }

    /// The most one request may write: Thorough's thinking counts against
    /// it on Anthropic, so it gets room for a lesson after the thinking.
    pub fn max_tokens(self) -> u32 {
        if self == Effort::Thorough { GENERATE_MAX_TOKENS * 2 } else { GENERATE_MAX_TOKENS }
    }

    /// A job's expected output at this level, from [`job_tokens`]'s: Thorough
    /// reserves twice as much, so it does not run past what it held.
    pub fn output_tokens(self, output: u64) -> u64 {
        if self == Effort::Thorough { output * 2 } else { output }
    }
}

/// `max_tokens` of a job request on `p`.
pub(super) fn max_tokens(p: &Provider) -> u32 {
    p.effort.map_or(GENERATE_MAX_TOKENS, Effort::max_tokens)
}

/// The effort in a Messages API body, when the job has one.
pub(super) fn anthropic(body: &mut serde_json::Value, p: &Provider) {
    if let Some(e) = p.effort {
        if let Some(o) = body.as_object_mut() {
            o.insert("output_config".into(), serde_json::json!({ "effort": e.id() }));
        }
    }
}

/// The effort in a chat completions body (OpenAI, and Gemini's
/// OpenAI-compatible endpoint), when the job has one.
pub(super) fn openai(body: &mut serde_json::Value, p: &Provider) {
    if let Some(e) = p.effort {
        if let Some(o) = body.as_object_mut() {
            o.insert("reasoning_effort".into(), e.id().into());
        }
    }
}
