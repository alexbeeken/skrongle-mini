//! "Generate the course" with AI, and following a job.

use super::*;

// ---------- generate with AI ----------

/// One provider on the form: whether a key is saved on the profile.
#[derive(Clone)]
pub struct ProviderChoice {
    pub id: &'static str,
    pub label: String,
    /// Last four characters of the saved key, when there is one.
    pub saved_hint: Option<String>,
}

/// "Generate the course" (`generate_form.html`), a dialog on the editor of
/// a draft (a fresh course where only names changed) or of a generation that
/// stopped (to resume it).
pub struct GenerateView {
    pub course_id: String,
    pub title: String,
    /// On the course list, for a course whose first generation stopped.
    pub stopped: bool,
    /// Show the dialog when the page loads: just created, `?generate=1`, or a refused form.
    pub open: bool,
    pub error: Option<String>,
    pub wish: String,
    pub notes: String,
    pub kind: String,
    pub provider: String,
    /// The providers with a saved key. The form offers a choice only
    /// between two or more; with one it just uses it.
    pub providers: Vec<ProviderChoice>,
    /// No key saved: the form is shown greyed
    /// out under a link to the profile, so nobody writes a prompt for nothing.
    pub no_key: bool,
    /// "full", "preview" or "outline" (#1119).
    pub depth: String,
    pub max_notes: usize,
    pub instructions: String,
    pub default_instructions: &'static str,
    pub instructions_edited: bool,
    /// Resuming this course's stopped generation rather than writing it.
    pub resume: bool,
    /// The names already given, which the AI keeps.
    pub named: courses::Draft,
    /// What each offered provider charges, for the cost estimate under
    /// *How much to write* (#443).
    pub rates: Vec<Rate>,
    /// The effort picked in the dialog beside Generate: the account's last
    /// choice (`users.ai_effort`), or the one a refused form posted (#918).
    pub effort: ai::Effort,
}

/// Seconds a lesson request and an outline request usually take, for the
/// Generate dialog's rough time (requests run one after another).
pub const LESSON_SECONDS: u64 = 45;
pub const OUTLINE_SECONDS: u64 = 30;

/// Canadian cents to the US dollar, for the estimate: providers list their
/// prices in US dollars, the app shows Canadian ones like its plans. Fixed,
/// since the estimate is rough anyway.
pub const CAD_CENTS_PER_USD: i64 = 140;

/// `usd` micro-dollars in Canadian micro-dollars, rounded up.
pub fn to_cad(usd: i64) -> i64 {
    (usd * CAD_CENTS_PER_USD + 99) / 100
}

/// About how many minutes a job of `lessons` lessons (plus an outline when
/// `plan`) takes, rounded up, at least 1.
pub fn job_minutes(lessons: usize, plan: bool) -> u64 {
    (lessons as u64 * LESSON_SECONDS + u64::from(plan) * OUTLINE_SECONDS).div_ceil(60).max(1)
}

/// One provider's prices for the Generate dialog's estimate, in
/// micro-dollars per million tokens. No prices: the model's price is not
/// known (only the time is shown).
#[derive(Clone, Debug, PartialEq)]
pub struct Rate {
    pub id: &'static str,
    pub price: Option<crate::prices::Price>,
}

impl Rate {
    /// The estimate's figures on the provider's radio button (or the hidden
    /// input with one provider), for static/courses.js to work it out again
    /// as the lesson count or the material changes.
    pub fn attrs(&self) -> String {
        let mut a = String::new();
        if let Some(p) = self.price {
            a += &format!(" data-price-in=\"{}\" data-price-out=\"{}\"", p.input, p.output);
        }
        a
    }
}

/// The prices behind each offered provider's estimate: the provider's
/// default model (the dialog has no model field).
pub(super) fn rates(state: &AppState, providers: &[ProviderChoice]) -> Vec<Rate> {
    let prices = &state.prices;
    let priced = |model: &str| prices.listed(model).then(|| prices.price(model));
    providers
        .iter()
        .map(|p| {
            let model = crate::secrets::Provider::parse(p.id).map(|k| ai::ProviderKind::from_secret(k).default_model());
            Rate { id: p.id, price: model.and_then(priced) }
        })
        .collect()
}

/// "6 lessons · about 5 minutes · roughly CA$1.48": the estimate of a job of `lessons` lessons and an
/// outline with `notes_chars` characters of material, on `rate`. The same
/// sums as `ai::job_tokens`; static/courses.js
/// redoes them in the browser.
pub fn estimate_line(lessons: usize, notes_chars: usize, rate: Option<&Rate>) -> String {
    let mut parts = vec![crate::tn("courses.js.estimate_lessons", lessons as i64), crate::tn("courses.js.estimate_minutes", job_minutes(lessons, true) as i64)];
    if let Some(p) = rate.and_then(|r| r.price) {
        let (input, output) = ai::job_tokens(lessons, true, notes_chars);
        let cost = crate::prices::PriceTable::cost_at(p, input, output);
        parts.push(crate::t1("courses.js.estimate_cost", "cost", format!("{:.2}", to_cad(cost).max(10_000) as f64 / 1_000_000.0)));
    }
    parts.join(&crate::t("courses.js.estimate_sep"))
}

impl GenerateView {
    /// The estimate as the dialog first shows it, for the provider first chosen.
    pub fn estimate(&self) -> String {
        estimate_line(self.depth_lessons(&self.depth), self.notes.trim().chars().count(), self.rates.iter().find(|r| r.id == self.provider))
    }

    /// The lessons the estimate counts for `depth` (`Depth::expected_lessons`),
    /// on each *How much to write* choice for static/gen-estimate.js.
    pub fn depth_lessons(&self, depth: &str) -> usize {
        ai::Depth::parse(depth).expected_lessons()
    }

    /// The sizes behind `ai::job_tokens` and `ai::job_minutes`, on the
    /// estimate line for static/courses.js.
    pub fn estimate_attrs(&self) -> String {
        format!(
            " data-format-chars=\"{}\" data-prompt-tokens=\"{}\" data-lesson-out=\"{}\" data-outline-out=\"{}\" data-lesson-secs=\"{}\" data-outline-secs=\"{}\" data-cad-cents=\"{}\"",
            ai::FORMAT_DOC.len(),
            ai::JOB_PROMPT_TOKENS,
            ai::LESSON_OUTPUT_TOKENS,
            ai::OUTLINE_OUTPUT_TOKENS,
            LESSON_SECONDS,
            OUTLINE_SECONDS,
            CAD_CENTS_PER_USD
        )
    }

    /// The estimate's figures for provider `id` (`Rate::attrs`).
    pub fn rate_attrs(&self, id: &str) -> String {
        self.rates.iter().find(|r| r.id == id).map(Rate::attrs).unwrap_or_default()
    }

    /// The effort choices beside Generate (#918).
    pub fn efforts(&self) -> [ai::Effort; 3] {
        ai::Effort::ALL
    }

    /// Say where the notes go, once there is a provider to send them to.
    pub fn discloses(&self) -> bool {
        !self.no_key
    }

    /// Who gets the notes, as the form first shows it.
    pub fn provider_label(&self) -> String {
        self.providers.iter().find(|p| p.id == self.provider).map(|p| p.label.to_string()).unwrap_or_else(|| crate::t("courses.generate.your_provider"))
    }

    /// "The AI keeps the title “T” and your lessons “a”, “b”.": the names
    /// already given, which the AI keeps (`generate_form.html`).
    pub fn named_text(&self) -> String {
        let quote = |s: &str| crate::t1("courses.generate.quoted", "text", s);
        let title = self.named.title.as_deref().map(|t| crate::t1("courses.generate.named_title", "title", quote(t)));
        let lessons = (!self.named.lessons.is_empty()).then(|| {
            let list = self.named.lessons.iter().map(|l| quote(l)).collect::<Vec<_>>().join(&crate::t("courses.generate.list_sep"));
            let form = if self.named.lessons.len() == 1 { "one" } else { "other" };
            crate::t1(&format!("courses.generate.named_lessons.{form}"), "list", list)
        });
        let what = match (title, lessons) {
            (Some(t), Some(l)) => crate::t2("courses.generate.named_and", "a", t, "b", l),
            (Some(t), None) => t,
            (None, Some(l)) => l,
            (None, None) => String::new(),
        };
        crate::t1("courses.generate.named_keeps", "what", what)
    }
}

/// The providers "Generate the course" can use: those the server has a key
/// for, in Manage Users' order.
pub(super) async fn offered_providers(state: &AppState, _user: &CurrentUser) -> Result<Vec<ProviderChoice>, AppError> {
    Ok(provider_choices(state).await?.into_iter().filter(|p| p.saved_hint.is_some()).collect())
}

/// The provider a form uses when nothing else is said: the first saved
/// key; `None`: no key at all.
fn default_provider(providers: &[ProviderChoice]) -> Option<&'static str> {
    providers.first().map(|p| p.id)
}

/// What "Generate the course" does on a course, if it is offered there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenMode {
    /// Write the whole course into it, keeping the names the learner gave.
    Fresh(courses::Draft),
    /// Write the lessons its stopped generation did not get to.
    Resume,
}

/// A draft, a generation that stopped before its outline (both: write the
/// course), or one that stopped after it (resume).
pub(super) async fn gen_mode(state: &AppState, row: &CourseRow) -> Result<Option<GenMode>, AppError> {
    // A stopped translation resumes from its card ("Copy and translate to…", #630).
    if row.source == "translate" {
        return Ok(None);
    }
    Ok(match row.state {
        courses::CourseState::Failed if row.outline.is_some() => Some(GenMode::Resume),
        courses::CourseState::Failed => Some(GenMode::Fresh(courses::Draft::default())),
        courses::CourseState::Ready if row.source == "hand" && row.error.is_none() => {
            let cid = row.id.clone();
            let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
            courses::draft_of(&files).map(GenMode::Fresh)
        }
        _ => None,
    })
}

/// The dialog as it first shows for `row`, or `None` when it is not offered.
pub(super) async fn generate_view(state: &AppState, user: &CurrentUser, row: &CourseRow, open: bool) -> Result<Option<GenerateView>, AppError> {
    if user.ai_off {
        return Ok(None);
    }
    let Some(mode) = gen_mode(state, row).await? else { return Ok(None) };
    let providers = offered_providers(state, user).await?;
    let provider = default_provider(&providers);
    let (resume, named) = match mode {
        GenMode::Resume => (true, courses::Draft::default()),
        GenMode::Fresh(d) => (false, d),
    };
    let stopped = row.state == courses::CourseState::Failed;
    // A retry starts from what the first run was asked.
    let notes = if stopped {
        let cid = row.id.clone();
        state.db.call(move |c| courses::notes_of(c, &cid)).await?
    } else {
        String::new()
    };
    let rates = rates(state, &providers);
    let error = row.error.as_deref().filter(|_| stopped).map(|e| ai::plain_failure(row.error_kind.as_deref(), e).unwrap_or_else(|| e.to_string()));
    Ok(Some(GenerateView {
        course_id: row.id.clone(),
        title: row.title.clone(),
        stopped,
        open,
        error,
        wish: row.brief.clone(),
        notes,
        kind: "auto".into(),
        provider: provider.unwrap_or_default().to_string(),
        providers,
        no_key: provider.is_none(),
        depth: ai::Depth::Full.as_str().into(),
        max_notes: ai::MAX_NOTES,
        instructions: ai::DEFAULT_INSTRUCTIONS.to_string(),
        default_instructions: ai::DEFAULT_INSTRUCTIONS,
        instructions_edited: false,
        resume,
        named,
        rates,
        effort: effort_of(state, user.id).await?,
    }))
}

#[derive(Deserialize)]
pub struct GenerateForm {
    pub provider: String,
    /// A key for a custom `base_url`, used for this job only and never
    /// stored: the server's keys are the owner's to set (#25).
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub wish: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub kind: String,
    /// "full" (default), "preview" or "outline".
    #[serde(default)]
    pub depth: String,
    #[serde(default)]
    pub instructions: String,
    /// `low`, `medium` or `high` (`ai::Effort::id`); missing or unknown: the account's last.
    #[serde(default)]
    pub effort: String,
}

/// The effort this account last generated a course at (`users.ai_effort`).
async fn effort_of(state: &AppState, user_id: i64) -> Result<ai::Effort, AppError> {
    let id: Option<String> = state.db.call(move |c| rusqlite::OptionalExtension::optional(c.query_row("SELECT ai_effort FROM users WHERE id = ?1", [user_id], |r| r.get(0)))).await?;
    Ok(id.as_deref().and_then(ai::Effort::parse).unwrap_or_default())
}

#[derive(Deserialize)]
pub struct GenerateParams {
    pub resume: Option<String>,
}

async fn provider_choices(state: &AppState) -> Result<Vec<ProviderChoice>, AppError> {
    let vault = state.vault.clone();
    let statuses = state.db.call(move |c| crate::secrets::statuses(c, &vault)).await?;
    Ok(crate::secrets::Provider::ALL
        .into_iter()
        .map(|p| ProviderChoice {
            id: p.id(),
            label: p.label().to_string(),
            saved_hint: statuses.iter().find(|k| k.provider == p).and_then(|k| k.hint.clone()),
        })
        .collect())
}

/// GET /courses/generate[?resume=<course id>]: the old form page. Courses
/// are generated from the editor's dialog now.
pub async fn generate_form(Query(q): Query<GenerateParams>) -> Response {
    let to = match q.resume.filter(|s| !s.is_empty()) {
        Some(id) => format!("/courses/{}?generate=1", crate::urlenc::urlencode(&id)),
        None => "/courses".into(),
    };
    Redirect::to(&crate::url(&to)).into_response()
}

/// Why this account may not start another AI job yet, if it may not: its
/// hourly or daily quota (`security::AI_JOBS_PER_USER_*`) is used up.
pub(super) fn ai_quota_problem(state: &AppState, user_id: i64) -> Option<String> {
    let (key, now) = (user_id.to_string(), crate::db::now());
    if state.limits.blocked(crate::security::AI_JOBS_PER_USER_HOUR, &key, now) {
        return Some(format!("You have started {} AI jobs in the last hour, the most an hour allows; try again later.", crate::security::AI_JOBS_PER_USER_HOUR.max));
    }
    if state.limits.blocked(crate::security::AI_JOBS_PER_USER_DAY, &key, now) {
        return Some(format!("You have started {} AI jobs today, the most a day allows; try again tomorrow.", crate::security::AI_JOBS_PER_USER_DAY.max));
    }
    None
}

/// Count a started AI job against the account's quota.
pub(super) fn count_ai_job(state: &AppState, user_id: i64) {
    let (key, now) = (user_id.to_string(), crate::db::now());
    let _ = state.limits.hit(crate::security::AI_JOBS_PER_USER_HOUR, &key, now);
    let _ = state.limits.hit(crate::security::AI_JOBS_PER_USER_DAY, &key, now);
}

/// The model to call, from the provider fields both AI forms send. The key
/// is the pasted one (checked like Manage Users checks it, unless a custom
/// base URL points at another server; kept in memory for the job only) or
/// the server's, which only goes to the provider's own address: a custom
/// base URL needs a pasted key. `Ok(Err(msg))` is a mistake to show on the form.
pub(super) async fn ai_provider(
    state: &AppState,
    _user: &CurrentUser,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Result<ai::Provider, String>, AppError> {
    let Some(secret_provider) = crate::secrets::Provider::parse(provider) else { return Ok(Err(crate::t("courses.generate.choose_provider"))) };
    let kind = ai::ProviderKind::from_secret(secret_provider);
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if !base_url.is_empty() && !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Ok(Err(crate::t("courses.generate.base_url")));
    }
    // The server, not the browser, calls this URL: keep it off internal addresses.
    if !base_url.is_empty() {
        if let Err(e) = ai::check_base_url(&base_url, ai::private_urls_allowed(state.dev_mode())).await {
            return Ok(Err(e));
        }
    }
    let pasted = api_key.trim().to_string();
    // A saved key only ever goes to its provider's own address: a custom
    // base URL gets the key pasted with it, never the profile's.
    if pasted.is_empty() && !base_url.is_empty() {
        return Ok(Err("A custom base URL needs its own API key: paste the key for that server. Saved keys are only sent to the provider's own address.".into()));
    }
    let model = model.trim().to_string();
    if pasted.is_empty() {
        // The server's: its key, or Ollama's base URL and model (#24).
        let vault = state.vault.clone();
        let Some(mut p) = state.db.call(move |c| ai::server_provider(c, &vault, kind, false)).await? else {
            return Ok(Err(crate::t1("courses.generate.paste_key", "provider", secret_provider.label())));
        };
        if !model.is_empty() {
            p.model = model;
        }
        return Ok(Ok(p));
    }
    let api_key = {
        if base_url.is_empty() {
            match crate::secrets::validate_key(secret_provider, &pasted) {
                Ok(k) => k,
                Err(e) => return Ok(Err(e)),
            }
        } else if pasted.len() > crate::secrets::KEY_MAX || pasted.chars().any(char::is_whitespace) {
            return Ok(Err(crate::t("courses.generate.bad_key")));
        } else {
            pasted
        }
    };
    Ok(Ok(ai::Provider {
        kind,
        base_url: if base_url.is_empty() { kind.default_base_url() } else { base_url },
        api_key,
        model: if model.is_empty() { kind.default_model().to_string() } else { model },
        effort: None,
    }))
}

/// The Generate dialog's refusal of a wish over `ai::MAX_WISH` (its script says the same).
pub fn wish_too_long() -> String {
    crate::t1("courses.generate.wish_too_long", "max", ai::grouped(ai::MAX_WISH))
}

/// The Generate dialog's refusal of notes over `ai::MAX_NOTES` (its script says the same).
pub fn notes_too_long() -> String {
    crate::t1("courses.generate.notes_too_long", "max", ai::grouped(ai::MAX_NOTES))
}

/// Why Use AI refuses `text` (trimmed), if it does: fewer than
/// `ai::MIN_PROMPT` characters, or more than the mode takes. The live count
/// under the box uses the same limits.
pub(super) fn ai_text_problem(text: &str, mode: AiMode) -> Option<String> {
    let change = mode != AiMode::Add;
    let max = if change { ai::MAX_FEEDBACK } else { ai::MAX_BRIEF };
    let n = text.chars().count();
    if n < ai::MIN_PROMPT {
        let what = if change { crate::t("courses.ai.say_change") } else { crate::t("courses.ai.say_add") };
        return Some(crate::t2("courses.ai.too_short", "what", what, "min", ai::MIN_PROMPT));
    }
    (n > max).then(|| crate::t1("courses.ai.too_long", "max", ai::grouped(max)))
}

/// A short provisional title from the learner's words, shown greyed out in
/// the course list until the outline brings the real one.
fn provisional_title(wish: &str) -> String {
    let new_course = crate::t("courses.generate.new_course");
    let first = wish.lines().find(|l| !l.trim().is_empty()).unwrap_or(&new_course).trim();
    let first = first.split(['.', '!', '?']).next().unwrap_or(first).trim();
    let mut t: String = first.chars().take(60).collect();
    if first.chars().count() > 60 {
        t = t.trim_end().to_string() + "…";
    }
    if t.is_empty() { new_course } else { t }
}

/// POST /courses/{id}/generate: the dialog's "Generate the course". A draft
/// is written over in place (same id), keeping the names the learner gave; a
/// stopped generation resumes.
pub async fn generate_in(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Form(form): Form<GenerateForm>,
) -> Result<Response, AppError> {
    user.require_ai()?;
    let row = owned(&state, &user, &id).await?;
    let Some(mode) = gen_mode(&state, &row).await? else {
        return Ok(to_editor(&id, if row.is_busy() { "busy" } else { "has-content" }));
    };
    let depth = ai::Depth::parse(&form.depth);
    let lessons = depth.expected_lessons();
    let providers = offered_providers(&state, &user).await?;
    let rates = rates(&state, &providers);
    let no_key = default_provider(&providers).is_none();
    // Browsers send textareas with CRLF line ends.
    let mut form = form;
    form.instructions = form.instructions.replace("\r\n", "\n");
    form.notes = form.notes.replace("\r\n", "\n");
    let instructions_edited = form.instructions.trim() != ai::DEFAULT_INSTRUCTIONS.trim() && !form.instructions.trim().is_empty();
    let named = match &mode {
        GenMode::Fresh(d) => d.clone(),
        GenMode::Resume => courses::Draft::default(),
    };
    let stopped = row.state == courses::CourseState::Failed;
    // What the learner picked last becomes the account's default (#918).
    let effort = match ai::Effort::parse(&form.effort) {
        Some(e) => {
            let (uid, id) = (user.id, e.id());
            state.db.call(move |c| c.execute("UPDATE users SET ai_effort = ?2 WHERE id = ?1", rusqlite::params![uid, id])).await?;
            e
        }
        None => effort_of(&state, user.id).await?,
    };
    let refused = |msg: String| GenerateView {
        course_id: row.id.clone(),
        title: row.title.clone(),
        stopped,
        open: true,
        error: Some(msg),
        wish: if mode == GenMode::Resume { row.brief.clone() } else { form.wish.clone() },
        notes: form.notes.clone(),
        kind: ai::SubjectKind::parse(&form.kind).as_str().to_string(),
        provider: form.provider.clone(),
        providers,
        no_key,
        depth: depth.as_str().into(),
        max_notes: ai::MAX_NOTES,
        instructions: if form.instructions.trim().is_empty() { ai::DEFAULT_INSTRUCTIONS.to_string() } else { form.instructions.clone() },
        default_instructions: ai::DEFAULT_INSTRUCTIONS,
        instructions_edited,
        resume: mode == GenMode::Resume,
        named: named.clone(),
        rates: rates.clone(),
        effort,
    };
    macro_rules! fail {
        ($msg:expr) => {{
            let view = refused($msg);
            if stopped {
                return picker_page(&state, user.clone(), None, Some(view)).await;
            }
            let row = owned(&state, &user, &id).await?;
            return editor_page_with(&state, user.clone(), row, None, Refill::default(), None, Some(view), None, false).await;
        }};
    }
    // A key pasted into the form lives only in memory: after a restart that
    // job keeps its Resume button rather than going on by itself (#874).
    let saved_key = form.api_key.trim().is_empty();
    let mut provider = match ai_provider(&state, &user, &form.provider, &form.api_key, &form.model, &form.base_url).await? {
        Ok(p) => p,
        Err(e) => fail!(e),
    };

    let wish = form.wish.trim().to_string();
    let notes = form.notes.trim().to_string();
    // Each box is empty or holds MIN_PROMPT characters or more (its live
    // count is red otherwise), and the two can't both be empty unless the
    // course has a title. static/courses.js checks the same before posting.
    let (w, n) = (wish.chars().count(), notes.chars().count());
    let short = |len: usize| len > 0 && len < ai::MIN_PROMPT;
    if mode != GenMode::Resume && (short(w) || short(n) || (w == 0 && n == 0 && named.title.is_none())) {
        fail!(ai::PROMPT_TOO_SHORT.into());
    }
    if w > ai::MAX_WISH {
        fail!(wish_too_long());
    }
    if n > ai::MAX_NOTES {
        fail!(notes_too_long());
    }
    if form.instructions.chars().count() > ai::MAX_INSTRUCTIONS {
        fail!(crate::t1("courses.generate.instructions_too_long", "max", ai::MAX_INSTRUCTIONS));
    }
    if let Err(e) = ai::check_capacity(&state.jobs, user.id) {
        fail!(e);
    }
    if let Some(e) = ai_quota_problem(&state, user.id) {
        fail!(e);
    }
    let job = ai::new_job_id();

    // The course stays in the course list, greyed out while it is being written.
    let (resume_outline, wish, notes) = match &mode {
        GenMode::Resume => {
            let cid = row.id.clone();
            let saved_notes = state.db.call(move |c| courses::notes_of(c, &cid)).await?;
            (row.outline.clone(), row.brief.clone(), saved_notes)
        }
        GenMode::Fresh(d) => {
            // A title alone is enough to ask for.
            let brief = match (wish.is_empty(), &d.title) {
                (false, _) => wish.clone(),
                (true, Some(t)) if notes.chars().count() < 20 => t.clone(),
                (true, _) => provisional_title(&notes),
            };
            let title = d.title.clone().unwrap_or_else(|| provisional_title(&brief));
            let (cid, b, n) = (row.id.clone(), brief.clone(), notes.clone());
            state.db.call(move |c| courses::restart_generation(c, &cid, &title, &b, &n)).await?;
            (None, brief, notes)
        }
    };
    // The same model at every level; only how hard it works changes.
    provider.effort = Some(effort);
    let course_id = row.id.clone();
    let (cid, j) = (course_id.clone(), job.clone());
    state.db.call(move |c| courses::set_generating(c, &cid, &j)).await?;
    state.courses.invalidate(&course_id);

    let req = ai::Request {
        user_id: user.id,
        provider,
        course_id: course_id.clone(),
        wish,
        lesson_count: lessons,
        depth,
        notes,
        kind: ai::SubjectKind::parse(&form.kind),
        instructions: if instructions_edited { form.instructions.clone() } else { String::new() },
        resume_outline,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: crate::i18n::snapshot(),
        named: named.clone(),
    };
    match ai::submit(&state, req, job.clone(), saved_key).await {
        Ok(job) => {
            count_ai_job(&state, user.id);
            tracing::info!("user {} started course generation {job} for {course_id}", user.id);
            Ok(Redirect::to(&crate::url(&format!("/courses/{course_id}?job={job}"))).into_response())
        }
        Err(e) => {
            let (cid, msg) = (course_id.clone(), e.clone());
            state.db.call(move |c| courses::finish_generation(c, &cid, Some(&msg))).await?;
            fail!(e)
        }
    }
}

/// POST /courses/{id}/cancel: stop the generation job after its current request.
pub async fn generate_cancel(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    match row.job_id.as_deref().and_then(|j| state.jobs.get(j)).filter(|j| !j.snapshot().done) {
        Some(job) => ai::cancel(&state, &job).await?,
        None if row.is_generating() => {
            // No live job behind it (cannot normally happen): just stop.
            let cid = id.clone();
            let cancelled = crate::t("courses.generate.cancelled");
            state.db.call(move |c| courses::finish_generation(c, &cid, Some(&cancelled))).await?;
        }
        None => {}
    }
    // Back to the editor, whose modal follows the job to its end.
    let job = row.job_id.as_deref().map(|j| format!("&job={j}")).unwrap_or_default();
    let notice = if row.state == courses::CourseState::Revising { "cancel-revision" } else { "cancel-generation" };
    Ok(Redirect::to(&crate::url(&format!("/courses/{id}?notice={notice}{job}"))).into_response())
}

fn job_of(state: &AppState, user: &CurrentUser, id: &str) -> Result<Arc<ai::Job>, AppError> {
    let job = state.jobs.get(id).ok_or(AppError::NotFound)?;
    if job.user_id != user.id {
        return Err(AppError::NotFound);
    }
    Ok(job)
}

/// GET /courses/generate/{job}: the old status page. Every job is followed
/// in the editor's modal now.
pub async fn generate_status(State(state): State<Arc<AppState>>, user: CurrentUser, Path(job): Path<String>) -> Result<Response, AppError> {
    user.require_ai()?;
    let j = job_of(&state, &user, &job)?;
    Ok(Redirect::to(&crate::url(&format!("/courses/{}?job={job}", j.course_id))).into_response())
}

/// GET /api/generate/{job}
pub async fn generate_api(State(state): State<Arc<AppState>>, user: CurrentUser, Path(job): Path<String>) -> Result<Json<ai::Status>, AppError> {
    user.require_ai_api()?;
    let j = job_of(&state, &user, &job).map_err(|_| AppError::Unauthorized)?;
    Ok(Json(state.jobs.status(&j)))
}
