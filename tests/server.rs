mod common;

use std::time::{Duration, Instant};

use common::*;
use futures::StreamExt;
use open_rally::server::{BindPlan, ServerController, ServerStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

fn plan(port: u16) -> BindPlan {
    BindPlan {
        addrs: vec!["127.0.0.1".parse().unwrap()],
        port,
    }
}

/// raw HTTP 로 로그인해서 `rally_session=<token>` 을 돌려준다.
async fn raw_login(port: u16) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let body = format!(r#"{{"password":"{PASSWORD}"}}"#);
    let req = format!(
        "POST /api/login HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 204"), "{out}");
    let line = out
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap();
    line.split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn start_serves_and_ws_receives_events() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, _d) = test_state();
    let mut ctl = ServerController::new(rt.handle().clone());
    let addrs = ctl.start(state.clone(), plan(0)).unwrap();
    let port = addrs[0].port();
    assert!(ctl.is_running());
    assert!(matches!(ctl.status(), ServerStatus::Running { .. }));

    rt.block_on(async {
        let cookie = raw_login(port).await;
        let mut req = format!("ws://127.0.0.1:{port}/ws")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert("cookie", cookie.parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        state
            .events
            .send(r#"{"type":"created"}"#.to_string())
            .unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("timed out waiting for ws event")
            .unwrap()
            .unwrap();
        assert_eq!(msg.into_text().unwrap().as_str(), r#"{"type":"created"}"#);

        // 서버를 끄면 서버 쪽에서 WebSocket 을 닫는다.
        let ws_task = tokio::spawn(async move {
            while let Some(Ok(m)) = ws.next().await {
                if m.is_close() {
                    return true;
                }
            }
            false
        });
        tokio::task::spawn_blocking(move || ctl.stop()).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(3), ws_task)
            .await
            .expect("server did not close ws")
            .unwrap());
    });
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn ws_without_session_is_rejected() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, _d) = test_state();
    let mut ctl = ServerController::new(rt.handle().clone());
    let port = ctl.start(state, plan(0)).unwrap()[0].port();
    rt.block_on(async {
        let req = format!("ws://127.0.0.1:{port}/ws")
            .into_client_request()
            .unwrap();
        let err = tokio_tungstenite::connect_async(req).await.err().unwrap();
        assert!(err.to_string().contains("401"), "{err}");
    });
    ctl.stop();
}

#[test]
fn start_fails_when_port_in_use() {
    let hold = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = hold.local_addr().unwrap().port();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, _d) = test_state();
    let mut ctl = ServerController::new(rt.handle().clone());
    let r = ctl.start(state, plan(port));
    assert!(r.is_err());
    assert!(matches!(ctl.status(), ServerStatus::Failed(_)));
    assert!(!ctl.is_running());
}

#[test]
fn stop_returns_with_idle_connection_open() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, _d) = test_state();
    let mut ctl = ServerController::new(rt.handle().clone());
    let addr = ctl.start(state.clone(), plan(0)).unwrap()[0];
    let _idle = std::net::TcpStream::connect(addr).unwrap();
    let t = Instant::now();
    ctl.stop();
    assert!(t.elapsed() < Duration::from_secs(6));
    assert!(matches!(ctl.status(), ServerStatus::Stopped));
    assert!(std::net::TcpStream::connect(addr).is_err());

    // 다시 켤 수 있다.
    let addr2 = ctl.start(state, plan(addr.port())).unwrap()[0];
    assert_eq!(addr2.port(), addr.port());
    assert!(std::net::TcpStream::connect(addr2).is_ok());
    ctl.stop();
}
