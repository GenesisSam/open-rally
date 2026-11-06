use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::auth::{password_matches, session_cookie};

use super::middleware::client_ip;
use super::AppState;

const INDEX_HTML: &str = include_str!("../../assets/index.html");
const APP_JS: &str = include_str!("../../assets/app.js");
const STYLE_CSS: &str = include_str!("../../assets/style.css");

pub const CSP: &str =
    "default-src 'self'; img-src 'self' blob:; media-src 'self'; connect-src 'self' ws: wss:";

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

pub async fn index() -> Response {
    let mut r = static_response("text/html; charset=utf-8", INDEX_HTML);
    r.headers_mut()
        .insert("content-security-policy", HeaderValue::from_static(CSP));
    r
}

pub async fn app_js() -> Response {
    static_response("application/javascript; charset=utf-8", APP_JS)
}

pub async fn style_css() -> Response {
    static_response("text/css; charset=utf-8", STYLE_CSS)
}

#[derive(Deserialize)]
pub struct LoginReq {
    password: String,
}

pub async fn login(State(state): State<AppState>, req: Request) -> Response {
    let ip = client_ip(&req);
    let now = Instant::now();
    if state.limiter.is_blocked(ip, now) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many attempts").into_response();
    }
    let Ok(Json(body)) = Json::<LoginReq>::from_request(req, &state).await else {
        return (StatusCode::BAD_REQUEST, "invalid body").into_response();
    };
    if password_matches(&state.password, &body.password) {
        state.limiter.record_success(ip);
        let token = state.sessions.issue();
        (
            StatusCode::NO_CONTENT,
            [(header::SET_COOKIE, session_cookie(&token))],
        )
            .into_response()
    } else {
        state.limiter.record_failure(ip, now);
        (StatusCode::UNAUTHORIZED, "wrong password").into_response()
    }
}

use axum::extract::FromRequest;

pub async fn session() -> StatusCode {
    StatusCode::NO_CONTENT
}
