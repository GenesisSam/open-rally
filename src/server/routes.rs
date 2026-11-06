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

// ---------- posts ----------

use axum::extract::{Multipart, Path, Query};
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::db::{NewAttachment, Post};
use crate::files::{guess_mime, kind_from_mime, sanitize_filename, unique_in_dir};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn bad_request(msg: impl ToString) -> Response {
    (StatusCode::BAD_REQUEST, msg.to_string()).into_response()
}

fn server_error(msg: impl ToString) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()).into_response()
}

#[derive(Deserialize)]
pub struct ListQuery {
    before: Option<String>,
    after: Option<String>,
    limit: Option<usize>,
}

pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 500;

pub async fn list_posts(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    match state
        .db
        .list_posts(q.before.as_deref(), q.after.as_deref(), limit)
    {
        Ok(posts) => Json(json!({ "posts": posts })).into_response(),
        Err(e) => server_error(e),
    }
}

pub async fn create_post(State(state): State<AppState>, mut mp: Multipart) -> Response {
    let post_id = state.next_id();
    let dir = state.uploads_dir.join(&post_id);
    match collect_post(&state, &post_id, &dir, &mut mp).await {
        Ok(post) => (StatusCode::CREATED, Json(post)).into_response(),
        Err(resp) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            resp
        }
    }
}

async fn collect_post(
    state: &AppState,
    post_id: &str,
    dir: &std::path::Path,
    mp: &mut Multipart,
) -> Result<Post, Response> {
    let mut body: Option<String> = None;
    let mut atts: Vec<NewAttachment> = Vec::new();

    while let Some(mut field) = mp.next_field().await.map_err(bad_request)? {
        match field.name().unwrap_or("") {
            "body" => {
                let text = field.text().await.map_err(bad_request)?;
                if !text.trim().is_empty() {
                    body = Some(text);
                }
            }
            "files" => {
                let raw = field.file_name().unwrap_or("").to_string();
                let mime = guess_mime(field.content_type(), &raw);
                tokio::fs::create_dir_all(dir).await.map_err(server_error)?;
                let filename = unique_in_dir(dir, &sanitize_filename(&raw));
                let mut file = tokio::fs::File::create(dir.join(&filename))
                    .await
                    .map_err(server_error)?;
                let mut size: u64 = 0;
                while let Some(chunk) = field.chunk().await.map_err(bad_request)? {
                    file.write_all(&chunk).await.map_err(server_error)?;
                    size += chunk.len() as u64;
                }
                file.flush().await.map_err(server_error)?;
                atts.push(NewAttachment {
                    id: state.next_id(),
                    filename,
                    size,
                    kind: kind_from_mime(&mime).to_string(),
                    mime,
                });
            }
            _ => while field.chunk().await.map_err(bad_request)?.is_some() {},
        }
    }

    if body.is_none() && atts.is_empty() {
        return Err(bad_request("텍스트나 파일이 필요합니다"));
    }
    let post = state
        .db
        .insert_post(post_id, now_ms(), body.as_deref(), &atts)
        .map_err(server_error)?;
    let _ = state
        .events
        .send(json!({ "type": "created", "post": post }).to_string());
    Ok(post)
}

pub async fn delete_post(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.db.delete_post(&id) {
        Ok(true) => {
            let _ = tokio::fs::remove_dir_all(state.uploads_dir.join(&id)).await;
            let _ = state
                .events
                .send(json!({ "type": "deleted", "id": id }).to_string());
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => server_error(e),
    }
}

// ---------- files ----------

use mime_guess::mime::{Mime, APPLICATION_OCTET_STREAM};
use tower_http::services::ServeFile;

use crate::files::disposition;

pub async fn serve_file(
    State(state): State<AppState>,
    Path((id, _name)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some((post_id, att)) = state.db.get_attachment(&id).ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state.uploads_dir.join(post_id).join(&att.filename);
    let mime: Mime = att.mime.parse().unwrap_or(APPLICATION_OCTET_STREAM);
    let mut svc = ServeFile::new_with_mime(path, &mime);
    let mut resp = match svc.try_call(req).await {
        Ok(r) => r.into_response(),
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let headers = resp.headers_mut();
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&disposition(&att.mime, &att.filename)) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}
