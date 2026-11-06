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
