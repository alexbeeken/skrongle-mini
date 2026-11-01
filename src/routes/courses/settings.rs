//! A course's settings dialog and wizard, and opening a course.

use super::*;

#[derive(Deserialize, Default)]
pub struct SettingsForm {
    /// '' for Default, or a theme id.
    pub theme: Option<String>,
    /// The profile picture in this course (`profile::AVATARS`).
    pub avatar: Option<String>,
    /// Its background: a palette slot (`profile::is_color_slot`).
    pub avatar_color: Option<u8>,
    /// The motivator worn in this course (`motivators::ALL`).
    pub motivator: Option<String>,
    /// The level to start at, for a course with levels: a unit id of
    /// `pages::level_options`, '' for the beginning (#587).
    pub start_unit: Option<String>,
    /// `home` from the first visit's wizard on the course home, which it
    /// goes back to; the course list otherwise.
    #[serde(default)]
    pub next: String,
    /// "1" from the wizard behind a course's Start button (#587): its
    /// settings, saved as the course opens.
    #[serde(default)]
    pub wizard: String,
    /// A lesson slug from a stat modal's "what's next" (#900): open the
    /// course at that lesson instead of its home.
    #[serde(default)]
    pub lesson: String,
}

/// Check a settings form against what the user may pick: a theme they may
/// not wear, or a picture, background, motivator or level that is not one,
/// is a 400.
async fn settings_change(state: &AppState, user: &CurrentUser, content: &ContentStore, form: &SettingsForm) -> Result<courses::SettingsChange, AppError> {
    let mut change = courses::SettingsChange::default();
    if let Some(t) = &form.theme {
        if !t.is_empty() && state.themes.resolve(&state.db, user.id, t).await?.is_none() {
            return Err(AppError::BadRequest(crate::t("profile.save.theme_unavailable")));
        }
        change.theme = Some(t.clone());
    }
    if let Some(a) = &form.avatar {
        let Some(avatar) = crate::profile::canonical(a) else {
            return Err(AppError::BadRequest(crate::t("profile.save.pick_picture")));
        };
        change.avatar = Some(avatar.to_string());
    }
    if let Some(slot) = form.avatar_color {
        if !crate::profile::is_color_slot(slot) {
            return Err(AppError::BadRequest(crate::t("profile.save.pick_colour")));
        }
        change.avatar_color = Some(slot);
    }
    if let Some(m) = &form.motivator {
        let Some(m) = crate::motivators::find(m) else {
            return Err(AppError::BadRequest(crate::t("profile.save.motivator_unavailable")));
        };
        change.motivator = Some(m.id().to_string());
    }
    if let Some(unit) = &form.start_unit {
        if !super::pages::level_options(content, 0).iter().any(|l| &l.id == unit) {
            return Err(AppError::BadRequest(crate::t1("study.start.no_such_level", "unit", unit)));
        }
        change.start_unit = Some(unit.clone());
    }
    Ok(change)
}

/// Save a checked settings change for a course the user opened; false when
/// they never opened it. A new level moves where every mode resumes to its
/// first lesson (#333); nothing is passed or scored, the lessons below it
/// are skipped (`progress::is_skipped`).
async fn save_settings(state: &AppState, user_id: i64, content: &ContentStore, change: courses::SettingsChange) -> Result<bool, AppError> {
    let cid = content.id().to_string();
    let first = change.start_unit.as_deref().and_then(|u| content.lessons_in(content.start_index(u)).next()).map(|l| l.id.clone());
    state
        .db
        .call(move |c| {
            let Some(pr) = courses::project(c, user_id, &cid)? else { return Ok(false) };
            let moved = change.start_unit.as_ref().is_some_and(|u| *u != pr.start_unit);
            courses::set_project_settings(c, user_id, &cid, &change)?;
            if let (true, Some(l)) = (moved, first) {
                tracing::info!("user {user_id} starts course {cid} at '{}'", change.start_unit.as_deref().unwrap_or_default());
                crate::activity::move_to(c, user_id, &cid, &l)?;
            }
            Ok(true)
        })
        .await
}

/// POST /courses/{id}/settings: a course's settings (#565, #574, #587),
/// from its dialog on the course list or the wizard of its first visit:
/// the level, the motivator and theme worn in it and the profile picture
/// shown there. A field left out keeps its value; any save, even of nothing
/// (the wizard closed), counts the course as set up. A choice that is not
/// one is a 400 (`settings_change`); a course they never opened a 404.
pub async fn set_settings(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<SettingsForm>) -> Result<Response, AppError> {
    let Some(content) = state.courses.get(&state.db, &id).await? else { return Err(AppError::NotFound) };
    let change = settings_change(&state, &user, &content, &form).await?;
    if !save_settings(&state, user.id, &content, change).await? {
        return Err(AppError::NotFound);
    }
    tracing::info!("user {} changed the settings of course {id}", user.id);
    Ok(Redirect::to(&crate::url(if form.next == "home" { "/" } else { "/courses" })).into_response())
}

/// Is this course one the user may open: built-in, their own, or shared with them?
pub(super) async fn accessible(state: &AppState, user: &CurrentUser, id: &str) -> Result<bool, AppError> {
    if state.courses.is_builtin(id) {
        return Ok(true);
    }
    let (uid, cid) = (user.id, id.to_string());
    state
        .db
        .call(move |c| {
            let Some(row) = courses::course(c, &cid)? else { return Ok(false) };
            if row.is_generating() {
                return Ok(false);
            }
            Ok(row.owner_id == uid || crate::share::can_open(c, uid, &cid)?)
        })
        .await
}

/// POST /courses/open/{id}: open a course, from its card's button. From the
/// wizard behind a Start button (`wizard=1`, #587) it carries the course's
/// settings, saved as it opens; without them (no script, the editor's open
/// icon) a course opened for the first time shows the wizard on its home.
pub async fn open(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    form: Result<Form<SettingsForm>, axum::extract::rejection::FormRejection>,
) -> Result<Response, AppError> {
    // A bare button posts no form at all.
    let form = form.map(|f| f.0).unwrap_or_default();
    if !accessible(&state, &user, &id).await? {
        return Err(AppError::NotFound);
    }
    // A course of their own that does not load: its editor says why.
    let Some(content) = state.courses.get(&state.db, &id).await? else {
        return Ok(Redirect::to(&crate::url(&format!("/courses/{id}"))).into_response());
    };
    let change = if form.wizard == "1" { Some(settings_change(&state, &user, &content, &form).await?) } else { None };
    let then = match content.lessons().iter().find(|l| !form.lesson.is_empty() && l.slug == form.lesson) {
        Some(l) => format!("/lessons/{}", l.slug),
        None => "/".to_string(),
    };
    open_for(&state, &user, &id, &format!("/courses/{id}"), &then).await?;
    if let Some(change) = change {
        save_settings(&state, user.id, &content, change).await?;
        tracing::info!("user {} set up course {id} from its wizard", user.id);
    }
    Ok(Redirect::to(&crate::url(&then)).into_response())
}

/// Open a course the user may open (checked by the caller): their project
/// in it, made active, then on to `then`, where a course opened for the
/// first time shows its settings wizard (`pages::home`). A course that does
/// not load sends them to `broken`.
pub(in crate::routes) async fn open_for(state: &AppState, user: &CurrentUser, id: &str, broken: &str, then: &str) -> Result<Response, AppError> {
    if state.courses.get(&state.db, id).await?.is_none() {
        return Ok(Redirect::to(&crate::url(broken)).into_response());
    }
    let (uid, cid) = (user.id, id.to_string());
    state.db.call(move |c| courses::open_project(c, uid, &cid)).await?;
    tracing::info!("user {} opened course {id}", user.id);
    Ok(Redirect::to(&crate::url(then)).into_response())
}

/// The course `id` for its placement test (#1106): one the user may open
/// that loads and has a test on offer.
async fn placement_course(state: &AppState, user: &CurrentUser, id: &str) -> Result<Arc<ContentStore>, AppError> {
    if !accessible(state, user, id).await? {
        return Err(AppError::NotFound);
    }
    match state.courses.get(&state.db, id).await? {
        Some(content) if crate::placement::offered(&content) => Ok(content),
        _ => Err(AppError::NotFound),
    }
}

/// GET /courses/{id}/placement: one sitting of the placement test, the
/// questions without their answers (`placement::draw`), for the start
/// wizard's level step (`static/placement.js`).
pub async fn placement(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    let content = placement_course(&state, &user, &id).await?;
    let questions = crate::placement::draw(&content, &mut crate::quiz::Rng::from_entropy());
    Ok(Json(serde_json::json!({ "questions": questions })).into_response())
}

#[derive(Deserialize)]
pub struct PlacementAnswer {
    pub id: String,
    pub answer: String,
}

#[derive(Deserialize)]
pub struct PlacementAnswers {
    pub answers: Vec<PlacementAnswer>,
}

/// POST /courses/{id}/placement {answers: [{id, answer}]}: the test marked,
/// each unit's tally and the unit to start at (`placement::mark`). Nothing
/// is stored: the wizard picks that unit, and Get Started! saves it.
pub async fn mark_placement(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Json(body): Json<PlacementAnswers>) -> Result<Response, AppError> {
    let content = placement_course(&state, &user, &id).await?;
    if body.answers.len() > 500 {
        return Err(AppError::ApiBadRequest("too many answers".into()));
    }
    let answers: Vec<(String, String)> = body.answers.into_iter().map(|a| (a.id, a.answer)).collect();
    let outcome = crate::placement::mark(&content, &answers).ok_or(AppError::NotFound)?;
    Ok(Json(outcome).into_response())
}
