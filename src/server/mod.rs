pub mod middleware;
pub mod routes;
pub mod ws;

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::broadcast;

use crate::auth::{RateLimiter, SessionStore};
use crate::db::Db;

pub struct Inner {
    pub db: Db,
    pub uploads_dir: PathBuf,
    pub password: String,
    pub sessions: SessionStore,
    pub limiter: RateLimiter,
    pub events: broadcast::Sender<String>,
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    pub fn new(db: Db, uploads_dir: PathBuf, password: String) -> AppState {
        AppState(Arc::new(Inner {
            db,
            uploads_dir,
            password,
            sessions: SessionStore::new(),
            limiter: RateLimiter::new(5, Duration::from_secs(10)),
            events: ws::event_channel(),
        }))
    }
}

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(routes::index))
        .route("/app.js", get(routes::app_js))
        .route("/style.css", get(routes::style_css))
        .route("/api/login", post(routes::login))
        .route("/api/session", get(routes::session))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_session,
        ))
        .layer(axum::middleware::from_fn(middleware::check_origin))
        .layer(axum::middleware::from_fn(middleware::check_host))
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}
