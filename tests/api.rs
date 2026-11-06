mod common;

use axum::body::Body;
use common::*;
use open_rally::server::build_router;
use tower::ServiceExt;

#[tokio::test]
async fn index_served_without_session_with_csp() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let r = app
        .oneshot(req("GET", "/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .starts_with("default-src 'self'"));
    assert!(r.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
}

#[tokio::test]
async fn bad_host_is_421_and_bad_origin_is_403() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let r = app
        .clone()
        .oneshot(
            req("GET", "/")
                .header("host", "evil.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 421);
    let r = app
        .oneshot(
            req("POST", "/api/login")
                .header("origin", "http://evil.com")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"pw"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
}

#[tokio::test]
async fn login_flow_and_rate_limit() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let r = app
        .clone()
        .oneshot(req("GET", "/api/session").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    for _ in 0..5 {
        let r = app
            .clone()
            .oneshot(
                req("POST", "/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"password":"wrong"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }
    let r = app
        .clone()
        .oneshot(
            req("POST", "/api/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"pw"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    // 다른 IP 는 통과
    let r = app
        .clone()
        .oneshot(
            req_from("POST", "/api/login", "10.0.0.9:1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"pw"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let cookie = r.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Max-Age=2592000"));
    let token = cookie.split(';').next().unwrap().to_string();
    let r = app
        .oneshot(
            req("GET", "/api/session")
                .header("cookie", token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
}

// ---------- Task 8: posts ----------

async fn post_multipart(
    app: &axum::Router,
    cookie: &str,
    parts: &[(&str, Option<&str>, &str, &[u8])],
) -> axum::http::Response<Body> {
    let (ct, body) = multipart(parts);
    app.clone()
        .oneshot(
            req("POST", "/api/posts")
                .header("cookie", cookie)
                .header("content-type", ct)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get_posts(app: &axum::Router, cookie: &str, query: &str) -> serde_json::Value {
    let r = app
        .clone()
        .oneshot(
            req("GET", &format!("/api/posts{query}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    body_json(r).await
}

async fn delete_post(app: &axum::Router, cookie: &str, id: &str) -> u16 {
    app.clone()
        .oneshot(
            req("DELETE", &format!("/api/posts/{id}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn posts_require_session() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let r = app
        .oneshot(req("GET", "/api/posts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn create_text_post_list_and_delete() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let text = "OPENAI_API_KEY=x\nB=2";
    let r = post_multipart(&app, &cookie, &[("body", None, "text/plain", text.as_bytes())]).await;
    assert_eq!(r.status(), 201);
    let created = body_json(r).await;
    assert_eq!(created["body"], text);
    assert_eq!(created["attachments"].as_array().unwrap().len(), 0);
    let id = created["id"].as_str().unwrap().to_string();

    let list = get_posts(&app, &cookie, "").await;
    assert_eq!(list["posts"].as_array().unwrap().len(), 1);
    assert_eq!(list["posts"][0]["id"], id);

    assert_eq!(delete_post(&app, &cookie, &id).await, 204);
    assert_eq!(delete_post(&app, &cookie, &id).await, 404);
    let list = get_posts(&app, &cookie, "").await;
    assert_eq!(list["posts"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn create_file_post_writes_to_disk_and_delete_removes_dir() {
    let (state, dir) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let big: Vec<u8> = (0..1_048_576u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
    let r = post_multipart(
        &app,
        &cookie,
        &[
            ("files", Some("deploy-tool"), "application/octet-stream", &big),
            ("files", Some("../x.png"), "image/png", b"png"),
        ],
    )
    .await;
    assert_eq!(r.status(), 201);
    let created = body_json(r).await;
    let id = created["id"].as_str().unwrap().to_string();
    assert!(created["body"].is_null());
    let atts = created["attachments"].as_array().unwrap();
    assert_eq!(atts[0]["filename"], "deploy-tool");
    assert_eq!(atts[0]["size"], 1_048_576);
    assert_eq!(atts[0]["kind"], "file");
    assert_eq!(atts[1]["filename"], "x.png");
    assert_eq!(atts[1]["kind"], "image");
    assert_eq!(atts[1]["mime"], "image/png");

    let post_dir = dir.path().join("uploads").join(&id);
    assert_eq!(std::fs::read(post_dir.join("deploy-tool")).unwrap(), big);
    assert_eq!(std::fs::read(post_dir.join("x.png")).unwrap(), b"png");

    assert_eq!(delete_post(&app, &cookie, &id).await, 204);
    assert!(!post_dir.exists());
}

#[tokio::test]
async fn empty_post_is_400() {
    let (state, dir) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let r = post_multipart(&app, &cookie, &[("body", None, "text/plain", b"   \n")]).await;
    assert_eq!(r.status(), 400);
    let r = post_multipart(&app, &cookie, &[]).await;
    assert_eq!(r.status(), 400);
    assert_eq!(std::fs::read_dir(dir.path().join("uploads")).map(|d| d.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn duplicate_filenames_in_one_post_get_suffix() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let r = post_multipart(
        &app,
        &cookie,
        &[
            ("files", Some("a.txt"), "text/plain", b"1"),
            ("files", Some("a.txt"), "text/plain", b"2"),
        ],
    )
    .await;
    assert_eq!(r.status(), 201);
    let created = body_json(r).await;
    let names: Vec<&str> = created["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["filename"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["a.txt", "a-2.txt"]);
}

#[tokio::test]
async fn create_post_cleans_up_on_truncated_body() {
    let (state, dir) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let (ct, mut body) = multipart(&[("files", Some("a.bin"), "application/octet-stream", &[7u8; 4096])]);
    let closing = format!("--{BOUNDARY}--\r\n");
    body.truncate(body.len() - closing.len() - 2);
    let r = app
        .clone()
        .oneshot(
            req("POST", "/api/posts")
                .header("cookie", &cookie)
                .header("content-type", ct)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(r.status().is_client_error() || r.status().is_server_error(), "{}", r.status());
    let list = get_posts(&app, &cookie, "").await;
    assert_eq!(list["posts"].as_array().unwrap().len(), 0);
    assert_eq!(std::fs::read_dir(dir.path().join("uploads")).map(|d| d.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn created_and_deleted_events_are_broadcast() {
    let (state, _d) = test_state();
    let mut rx = state.events.subscribe();
    let app = build_router(state);
    let cookie = login(&app).await;
    let r = post_multipart(&app, &cookie, &[("body", None, "text/plain", b"hi")]).await;
    let id = body_json(r).await["id"].as_str().unwrap().to_string();
    delete_post(&app, &cookie, &id).await;
    let a: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
    let b: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
    assert_eq!(a["type"], "created");
    assert_eq!(a["post"]["id"], id);
    assert_eq!(b["type"], "deleted");
    assert_eq!(b["id"], id);
}

#[tokio::test]
async fn list_pagination_before_after_limit() {
    let (state, _d) = test_state();
    let app = build_router(state);
    let cookie = login(&app).await;
    let mut ids = vec![];
    for i in 0..3 {
        let r = post_multipart(&app, &cookie, &[("body", None, "text/plain", format!("{i}").as_bytes())]).await;
        ids.push(body_json(r).await["id"].as_str().unwrap().to_string());
    }
    let list = get_posts(&app, &cookie, "?limit=2").await;
    let got: Vec<&str> = list["posts"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(got, [ids[2].as_str(), ids[1].as_str()]);
    let list = get_posts(&app, &cookie, &format!("?before={}", ids[1])).await;
    assert_eq!(list["posts"][0]["id"], ids[0]);
    let list = get_posts(&app, &cookie, &format!("?after={}", ids[1])).await;
    assert_eq!(list["posts"].as_array().unwrap().len(), 1);
    assert_eq!(list["posts"][0]["id"], ids[2]);
}
