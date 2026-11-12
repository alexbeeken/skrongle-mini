//! "What would you like to learn?": the course picker, projects, and the two
//! ways to make a course: by hand (an in-app file editor + upload) or with an
//! LLM and the learner's own API key.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{pages, time_ago, Page};
use crate::ai;
use crate::auth::CurrentUser;
use crate::content::{ContentStore, CourseFile, MANIFEST_FILE};
use crate::courses::{self, CourseRow};
use crate::review::{Edits, Exclusions};
use std::collections::HashMap;
use crate::db::now;
use crate::error::AppError;
use crate::AppState;

mod create;
mod editor;
mod files;
mod generate;
mod lessons;
mod picker;
mod revise;
mod settings;
mod stat;
mod write;

pub use create::*;
pub use editor::*;
pub use files::*;
pub use generate::*;
pub use lessons::*;
pub use picker::*;
pub use revise::*;
pub use settings::*;
pub use stat::*;
pub(crate) use write::*;

#[cfg(test)]
mod tests;
