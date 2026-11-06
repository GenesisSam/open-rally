use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::COOKIE_NAME;
use crate::net;

use super::AppState;

/// 세션 없이 열리는 경로.
const SESSION_EXEMPT: &[&str] = &["/", "/app.js", "/style.css", "/api/login"];

pub fn client_ip(req: &Request) -> IpAddr {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

pub fn session_token(req: &Request) -> Option<String> {
    let cookies = req.headers().get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == COOKIE_NAME).then(|| v.to_string())
    })
}

pub async fn check_host(req: Request, next: Next) -> Response {
    // Host 헤더가 여러 개면 전부 허용돼야 한다.
    let mut hosts = req.headers().get_all(header::HOST).iter().peekable();
    let ok = hosts.peek().is_some()
        && hosts.all(|v| v.to_str().map(net::host_allowed).unwrap_or(false));
    if !ok {
        return (StatusCode::MISDIRECTED_REQUEST, "host not allowed").into_response();
    }
    next.run(req).await
}

pub async fn check_origin(req: Request, next: Next) -> Response {
    let mutating = matches!(*req.method(), Method::POST | Method::DELETE | Method::PUT)
        || req.uri().path() == "/ws";
    if mutating {
        if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            if !net::origin_allowed(origin) {
                return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
            }
        }
    }
    next.run(req).await
}

pub async fn require_session(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if SESSION_EXEMPT.contains(&req.uri().path()) {
        return next.run(req).await;
    }
    match session_token(&req) {
        Some(t) if state.sessions.is_valid(&t) => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, "login required").into_response(),
    }
}
