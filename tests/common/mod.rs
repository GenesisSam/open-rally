#![allow(dead_code)]

use std::net::SocketAddr;

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{request::Builder, Request, Response};
use axum::Router;
use http_body_util::BodyExt;
use open_rally::db::Db;
use open_rally::server::AppState;
use tempfile::TempDir;
use tower::ServiceExt;

pub const PASSWORD: &str = "pw";

pub fn test_state() -> (AppState, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let state = AppState::new(db, dir.path().join("uploads"), PASSWORD.to_string());
    (state, dir)
}

pub fn req_from(method: &str, uri: &str, peer: &str) -> Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:7777")
        .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()))
}

pub fn req(method: &str, uri: &str) -> Builder {
    req_from(method, uri, "127.0.0.1:5000")
}

/// 로그인 후 `rally_session=<token>` 형태의 쿠키 값을 돌려준다.
pub async fn login(app: &Router) -> String {
    let r = app
        .clone()
        .oneshot(
            req("POST", "/api/login")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"password":"{PASSWORD}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 204, "login failed");
    let cookie = r.headers()["set-cookie"].to_str().unwrap();
    cookie.split(';').next().unwrap().to_string()
}

pub async fn body_bytes(resp: Response<Body>) -> Bytes {
    resp.into_body().collect().await.unwrap().to_bytes()
}

pub async fn body_json(resp: Response<Body>) -> serde_json::Value {
    serde_json::from_slice(&body_bytes(resp).await).unwrap()
}

pub const BOUNDARY: &str = "XrallyBoundaryX";

/// `(name, filename, mime, bytes)` 목록으로 multipart 본문을 만든다. `(content-type, body)`.
pub fn multipart(parts: &[(&str, Option<&str>, &str, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, filename, mime, bytes) in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match filename {
            Some(f) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\n")
                    .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}
