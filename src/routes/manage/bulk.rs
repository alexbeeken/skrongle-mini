//! Manage Users: one preset for several accounts at once (#141). The
//! accounts table is one form (`manage-bulk-form`): a checkbox per row the
//! viewer may edit, a bar above it with the presets, and a confirm dialog
//! (`static/manage.js`). The server decides who is skipped: an owner, the
//! signed-in account and an address nobody has are left as they are.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use std::sync::Arc;

use super::{ManageTemplate, ShowQuery};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::perms;
use crate::AppState;

/// POST /manage/permissions/bulk (email=… once per ticked account,
/// preset=<id>): every account named gets the preset's set
/// (`perms::apply_preset`, a copy), in one transaction. Skipped, and
/// counted: an address nobody has, an owner, and the signed-in account. An
/// owner or a superuser; a 404 otherwise. No address: back to the page with
/// an error. A preset that does not exist: a 400, nothing changed.
pub async fn set_permissions_bulk(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    if !user.manages_users() {
        return Err(AppError::NotFound);
    }
    let mut emails: Vec<String> = Vec::new();
    for (_, v) in form.iter().filter(|(k, _)| k == "email") {
        let email = v.trim().to_lowercase();
        if !email.is_empty() && !emails.contains(&email) {
            emails.push(email);
        }
    }
    if emails.is_empty() {
        return Ok(Redirect::to(&crate::url("/manage?did=bulk-none")).into_response());
    }
    let preset = form.iter().find(|(k, _)| k == "preset").map(|(_, v)| v.as_str()).unwrap_or_default();
    let Ok(preset) = preset.parse::<i64>() else {
        return Err(AppError::BadRequest("preset must be a preset's id".into()));
    };
    let viewer = user.id;
    let counts = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            if perms::preset(&tx, preset)?.is_none() {
                return Ok(None);
            }
            let (mut changed, mut skipped) = (0usize, 0usize);
            for email in &emails {
                let target = match crate::owner::find(&tx, email)? {
                    Some(id) if id != viewer => id,
                    _ => {
                        skipped += 1;
                        continue;
                    }
                };
                let owner: bool = tx.query_row("SELECT owner FROM users WHERE id = ?1", [target], |r| r.get(0))?;
                if owner {
                    skipped += 1;
                } else {
                    perms::apply_preset(&tx, target, preset)?;
                    changed += 1;
                }
            }
            tx.commit()?;
            Ok(Some((changed, skipped)))
        })
        .await?;
    let Some((changed, skipped)) = counts else {
        return Err(AppError::BadRequest("no such preset".into()));
    };
    tracing::info!("user {} gave preset {preset} to {changed} accounts ({skipped} skipped)", user.id);
    Ok(Redirect::to(&crate::url(&format!("/manage?did=bulk&n={changed}&skipped={skipped}&preset={preset}"))).into_response())
}

/// The page's notice after `did=bulk`: what was given to how many, and how
/// many were left as they are. The preset's name is the one the page lists.
pub(super) fn notice(t: &mut ManageTemplate, q: &ShowQuery) {
    let number = |v: &Option<String>| v.as_deref().and_then(|v| v.parse::<i64>().ok());
    let name = number(&q.preset).and_then(|id| t.presets.iter().find(|p| p.id == id)).map(|p| p.label.clone()).unwrap_or_default();
    let mut text = crate::i18n::fill(&crate::tn("manage.bulk_done", number(&q.n).unwrap_or(0)), &[("preset", &name)]);
    let skipped = number(&q.skipped).unwrap_or(0);
    if skipped > 0 {
        text.push(' ');
        text.push_str(&crate::tn("manage.bulk_skipped", skipped));
    }
    t.notice = Some(text);
}

#[cfg(test)]
mod tests {
    use crate::perms::{self, Perm, Perms};

    /// One preset for several accounts (#141): the accounts named get the
    /// preset's set and id; an owner, the signed-in account and an address
    /// nobody has are skipped and counted; a Student gets a 404; an empty
    /// selection or an unknown preset changes nothing. The page has one
    /// checkbox per row the viewer may edit, and no form inside the form.
    #[test]
    fn several_accounts_get_a_preset_at_once() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let state = crate::routes::test_state(false);
        let (root, sue, ann, bob, stu) = state
            .db
            .with(|c| {
                let root = crate::auth::create_user(c, "root@example.org", "h")?;
                crate::owner::make_owner(c, root)?;
                let sue = crate::auth::create_user(c, "sue@example.org", "h")?;
                perms::set_custom(c, sue, Perms::TEACHER.with(Perm::Superuser, true))?;
                let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
                perms::apply_preset(c, ann, perms::TEACHER_PRESET)?;
                let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
                perms::set_custom(c, bob, Perms::STUDENT.with(Perm::AiTutor, true))?;
                let stu = crate::auth::create_user(c, "stu@example.org", "h")?;
                perms::apply_preset(c, stu, perms::STUDENT_PRESET)?;
                Ok((root, sue, ann, bob, stu))
            })
            .unwrap();
        let cookie = |u| format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap());
        let (root_c, sue_c, stu_c) = (cookie(root), cookie(sue), cookie(stu));
        let stored = |u: i64| state.db.with(|c| c.query_row("SELECT perms, preset_id FROM users WHERE id = ?1", [u], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))).unwrap();
        let app = crate::routes::router(state.clone());
        rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        let base = format!("http://{addr}");
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let get = |path: &str, cookie: &str| {
            let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
        };
        // (status, Location)
        let post = |cookie: &str, form: &[(&str, &str)]| {
            let r = agent.post(&format!("{base}/manage/permissions/bulk")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
            (r.status().as_u16(), r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string())
        };
        let before = (stored(root), stored(sue), stored(ann), stored(bob), stored(stu));
        let all = || (stored(root), stored(sue), stored(ann), stored(bob), stored(stu));
        let teacher = (63, Some(perms::TEACHER_PRESET));
        let student = (0, Some(perms::STUDENT_PRESET));
        assert_eq!((before.2, before.3), (teacher, (4, None)));

        // A Student: a 404, nothing changed.
        assert_eq!(post(&stu_c, &[("email", "ann@example.org"), ("email", "bob@example.org"), ("preset", "1")]).0, 404);
        assert_eq!(all(), before);

        // Nothing ticked: back to the page with an error, nothing changed.
        for form in [&[("preset", "1")][..], &[("email", " "), ("preset", "1")][..]] {
            let (status, loc) = post(&root_c, form);
            assert_eq!((status, loc.as_str()), (303, "/manage?did=bulk-none"));
            let (_, page) = get(&loc, &root_c);
            assert!(page.contains(r#"data-testid="manage-error""#) && page.contains("Select at least one account first."), "{page}");
        }
        // No such preset, none named, or Custom: a 400, nothing changed.
        for preset in [("preset", "99"), ("preset", "custom"), ("other", "1")] {
            assert_eq!(post(&root_c, &[("email", "ann@example.org"), preset]).0, 400, "{preset:?}");
        }
        assert_eq!(all(), before);

        // The owner ticks three, an owner's among them: two changed, one skipped.
        let (status, loc) = post(&root_c, &[("email", "ann@example.org"), ("email", "root@example.org"), ("email", " BOB@example.org "), ("preset", "1")]);
        assert_eq!((status, loc.as_str()), (303, "/manage?did=bulk&n=2&skipped=1&preset=1"));
        assert_eq!((stored(ann), stored(bob)), (student, student));
        assert_eq!((stored(root), stored(sue), stored(stu)), (before.0, before.1, before.4));
        let (_, page) = get(&loc, &root_c);
        assert!(page.contains(r#"data-testid="manage-notice""#), "{page}");
        assert!(page.contains("&#34;Student&#34; given to 2 accounts. 1 was left as it is (owners and your own account cannot be changed)."), "{page}");

        // A superuser: their own account, an owner's and an address nobody
        // has are skipped; an address sent twice counts once.
        let (status, loc) = post(&sue_c, &[("preset", "2"), ("email", "sue@example.org"), ("email", "stu@example.org"), ("email", "stu@example.org"), ("email", "root@example.org"), ("email", "nobody@example.org")]);
        assert_eq!((status, loc.as_str()), (303, "/manage?did=bulk&n=1&skipped=3&preset=2"));
        assert_eq!(stored(stu), teacher);
        assert_eq!((stored(root), stored(sue)), (before.0, before.1));
        assert!(get(&loc, &sue_c).1.contains("&#34;Teacher&#34; given to 1 account. 3 were left as they are"));
        // Nobody skipped: the notice says only what was given.
        let (_, loc) = post(&sue_c, &[("email", "ann@example.org"), ("email", "bob@example.org"), ("preset", "2")]);
        assert_eq!(loc, "/manage?did=bulk&n=2&skipped=0&preset=2");
        let (_, page) = get(&loc, &sue_c);
        assert!(page.contains("&#34;Teacher&#34; given to 2 accounts.") && !page.contains("left as"), "{page}");
        assert_eq!((stored(ann), stored(bob)), (teacher, teacher));

        // The page: a checkbox on the rows the viewer may edit, the bar with
        // every preset and no Custom, and the table's form holds no other form.
        for (cookie, boxes) in [(&root_c, 4), (&sue_c, 3)] {
            let (_, page) = get("/manage", cookie);
            assert_eq!(page.matches(r#"data-testid="manage-account-select""#).count(), boxes, "{page}");
            assert!(!page.contains(r#"name="email" value="root@example.org""#), "{page}");
            assert_eq!(page.contains(r#"name="email" value="sue@example.org""#), boxes == 4);
            let form = page.split(r#"data-testid="manage-bulk-form""#).nth(1).and_then(|rest| rest.split("</form>").next()).unwrap_or_default();
            assert!(form.contains(r#"data-testid="manage-accounts""#) && form.contains(r#"data-testid="manage-bulk-bar""#) && form.contains(r#"data-testid="manage-account-select-all""#), "{form}");
            assert!(!form.contains("<form") && !form.contains("<dialog"), "a form inside the table's form: {form}");
            let select = form.split(r#"data-testid="manage-bulk-preset""#).nth(1).and_then(|rest| rest.split("</select>").next()).unwrap_or_default();
            assert!(select.contains(r#"<option value="1">Student</option>"#) && select.contains(r#"<option value="2">Teacher</option>"#) && !select.contains("custom"), "{select}");
            assert!(page.contains(r#"form="manage-bulk-form""#) && page.contains(r#"data-testid="manage-bulk-dialog""#), "{page}");
        }
    }
}
