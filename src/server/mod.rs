pub mod middleware;
pub mod routes;
pub mod ws;

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
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
    ids: Mutex<ulid::Generator>,
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
            ids: Mutex::new(ulid::Generator::new()),
        }))
    }

    /// 같은 밀리초 안에서도 단조 증가하는 ULID.
    pub fn next_id(&self) -> String {
        let mut g = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        g.generate().unwrap_or_else(|_| ulid::Ulid::generate()).to_string()
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
        .route("/api/posts", get(routes::list_posts).post(routes::create_post))
        .route("/api/posts/{id}", delete(routes::delete_post))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_session,
        ))
        .layer(axum::middleware::from_fn(middleware::check_origin))
        .layer(axum::middleware::from_fn(middleware::check_host))
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}
