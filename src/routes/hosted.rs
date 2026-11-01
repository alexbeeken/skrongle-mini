//! Skrongle AI on the pages: whether the server's key is offered to a
//! learner, the provider for one call on it, "AI left this month", and what
//! a learner can do when it does not cover a job (`OutOfAi`). The metering
//! itself is `metering::Meter`; the calls go through the same `ai` client
//! as a learner's own key.

use std::sync::Arc;

use crate::ai;
use crate::auth::CurrentUser;
use crate::billing::Plan;
use crate::error::AppError;
use crate::metering::{self, AiStatus, CreditPack, Need};
use crate::AppState;

/// When a learner asks for Skrongle AI the server does not offer them.
pub fn not_offered() -> String {
    crate::t1("courses.hosted.not_offered", "hosted", crate::tutor::hosted_label())
}

/// When a Plus learner asks for their own key while "use my keys when my
/// Skrongle AI limit is hit" is off.
pub fn own_keys_off() -> String {
    crate::t1("courses.hosted.own_keys_off", "hosted", crate::tutor::hosted_label())
}

/// This month's AI when the server offers its key to this learner for
/// `need`: the plan includes some (Plus, `SKRONGLE_AI_ALLOWANCE_PLUS` above
/// 0) or they hold credits (any plan, #931), and that is enough for the
/// smallest request of `need`
/// (`Meter::cheapest`). `None`: Skrongle AI is not offered to them for it
/// (#283); `NotIncluded` says why on the page.
pub async fn hosted_offer(state: &AppState, user_id: i64, need: Need) -> Result<Option<AiStatus>, AppError> {
    if !state.meter.hosted() {
        return Ok(None);
    }
    let s = status(state, user_id).await?;
    Ok(s.covers(state.meter.cheapest(need)).then_some(s))
}

/// A provider on the server's key for `purpose`, when it is offered to this learner for `need`.
pub async fn hosted_provider(state: &AppState, user_id: i64, purpose: &str, need: Need) -> Result<Option<ai::Provider>, AppError> {
    Ok(hosted_offer(state, user_id, need).await?.and_then(|_| state.meter.provider(purpose)))
}

/// Why Skrongle AI is not offered to this learner for a use while the
/// server has it (#283): the plan includes none, or less than the smallest
/// request of that use. The tutor's "add an API key" page (and the modal
/// the home page's tutor card opens instead of it) and the Generate dialog
/// say why and what to do instead (`not_included.html`).
#[derive(Clone, Debug)]
pub struct NotIncluded {
    pub need: Need,
    /// The plan includes no Skrongle AI at all (and there are no credits).
    pub none: bool,
    /// Plus's price, for a learner not on Plus when Plus's allowance would cover it.
    pub upgrade: Option<String>,
    /// On Plus with "use my keys when my Skrongle AI limit is hit" off: their
    /// own keys are not used until it is on.
    pub pill_off: bool,
}

impl NotIncluded {
    /// `None` when Skrongle AI is offered for `need`, or the server has no key of its own.
    pub async fn of(state: &AppState, user: &CurrentUser, need: Need) -> Result<Option<NotIncluded>, AppError> {
        if !state.meter.hosted() {
            return Ok(None);
        }
        let s = status(state, user.id).await?;
        let cheapest = state.meter.cheapest(need);
        if s.covers(cheapest) {
            return Ok(None);
        }
        let plus_covers = state.meter.allowance(Plan::Plus) >= cheapest;
        Ok(Some(NotIncluded {
            need,
            none: s.total() <= 0,
            upgrade: (user.plan != Plan::Plus && plus_covers).then(|| Plan::Plus.price_label()),
            pill_off: !own_keys_usable(state, user),
        }))
    }

    /// The first line: why there is no Skrongle AI here.
    pub fn reason(&self) -> String {
        let hosted = crate::tutor::hosted_label();
        let what = match self.need {
            Need::Tutor => "tutor",
            Need::Course => "course",
            Need::Lesson => "lesson",
        };
        if self.none {
            crate::t1("courses.hosted.gap.none", "hosted", hosted)
        } else {
            crate::t1(&format!("courses.hosted.gap.too_little_{what}"), "hosted", hosted)
        }
    }

    /// What the learner can do instead: their own key (with the pill on, on
    /// Plus), or Plus.
    pub fn instead(&self) -> String {
        let hosted = crate::tutor::hosted_label();
        match (self.pill_off, self.upgrade.is_some()) {
            (true, _) => crate::t1("courses.hosted.gap.instead_pill", "hosted", hosted),
            (false, true) => crate::t1("courses.hosted.gap.instead_upgrade", "hosted", hosted),
            (false, false) => crate::t("courses.hosted.gap.instead_key"),
        }
    }
}

pub async fn status(state: &AppState, user_id: i64) -> Result<AiStatus, AppError> {
    let meter = Arc::clone(&state.meter);
    state.db.call(move |c| meter.status(c, user_id)).await
}

/// "Skrongle AI left this month: 83 %" (allowance plus credits, less what
/// running jobs hold). Learners never see dollar amounts. `None` with plans
/// off (Skrongle mini): there is no monthly allowance to count down.
pub fn left_line(s: &AiStatus) -> Option<String> {
    (!s.unmetered).then(|| crate::t2("courses.hosted.left", "hosted", crate::tutor::hosted_label(), "n", s.percent_left()))
}

/// Plus learners have Skrongle AI first; their own keys only when "use my
/// keys when my Skrongle AI limit is hit" is on. Everyone else uses their own
/// keys as before (Skrongle AI after them, when they hold credits). With
/// plans off there is no switch: own keys always work.
pub fn own_keys_usable(state: &AppState, user: &CurrentUser) -> bool {
    !crate::billing::plans_on(state) || !(state.meter.hosted() && user.plan == Plan::Plus) || user.own_keys_fallback
}

/// Skrongle AI comes before the learner's own keys: Plus, with the server's key on.
pub fn hosted_first(state: &AppState, user: &CurrentUser) -> bool {
    state.meter.hosted() && user.plan == Plan::Plus
}

/// `left_line` for this learner, read now.
pub async fn ai_left(state: &AppState, user_id: i64) -> Result<Option<String>, AppError> {
    Ok(left_line(&status(state, user_id).await?))
}

/// What one generated lesson takes of this month's Skrongle AI, about: "6 %".
pub fn per_lesson(state: &AppState, s: &AiStatus) -> String {
    let (input, output) = ai::job_tokens(1, false, 0);
    crate::t1("courses.hosted.percent", "n", s.percent_of(state.meter.estimate(&state.meter.cfg.courses_model, input, output)))
}

/// The ways on when Skrongle AI does not cover a job or a turn: upgrade to
/// Plus (a learner not on it), a credit pack (when packs are sold, on any
/// plan, #931), or the learner's own key (always).
#[derive(Clone, Debug, Default)]
pub struct OutOfAi {
    /// Plus's price, "CA$15 a month", for a learner not on Plus.
    pub upgrade: Option<String>,
    pub packs: Vec<CreditPack>,
}

impl OutOfAi {
    pub fn of(state: &AppState, plan: Plan) -> OutOfAi {
        OutOfAi {
            upgrade: (plan != Plan::Plus).then(|| Plan::Plus.price_label()),
            packs: if state.meter.credits_offered() { state.meter.cfg.packs.clone() } else { Vec::new() },
        }
    }

    /// "$3.00": what a pack adds.
    pub fn credit(pack: &CreditPack) -> String {
        metering::dollars(pack.credit_micros)
    }
}
