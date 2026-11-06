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
use tokio::sync::{broadcast, watch};

use crate::auth::{RateLimiter, SessionStore};
use crate::db::Db;

pub struct Inner {
    pub db: Db,
    pub uploads_dir: PathBuf,
    pub password: String,
    pub sessions: SessionStore,
    pub limiter: RateLimiter,
    pub events: broadcast::Sender<String>,
    /// 서버를 끌 때 true 가 된다. 열린 WebSocket 이 이걸 보고 닫는다.
    pub shutdown: watch::Sender<bool>,
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
            shutdown: watch::channel(false).0,
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
        .route(
            "/api/posts",
            get(routes::list_posts)
                .post(routes::create_post)
                // 업로드만 무제한. 그 밖의 경로(특히 비인증 /api/login)는 axum 기본 2MB.
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/api/posts/{id}", delete(routes::delete_post))
        .route("/files/{id}/{name}", get(routes::serve_file))
        .route("/ws", get(ws::ws_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_session,
        ))
        .layer(axum::middleware::from_fn(middleware::check_origin))
        .layer(axum::middleware::from_fn(middleware::check_host))
        .with_state(state)
}

// ---------- ServerController ----------

use std::net::{IpAddr, SocketAddr};

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

pub struct BindPlan {
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub enum ServerStatus {
    Stopped,
    Running { addrs: Vec<SocketAddr> },
    Failed(String),
}

/// 같은 프로세스 안에서 HTTP 서버를 켜고 끈다. 모든 메서드는 동기이며 런타임 밖 스레드에서 부른다.
pub struct ServerController {
    rt: Handle,
    status: ServerStatus,
    shutdown: Option<watch::Sender<bool>>,
    ws_shutdown: Option<watch::Sender<bool>>,
    handles: Vec<JoinHandle<()>>,
}

pub const STOP_TIMEOUT: Duration = Duration::from_secs(5);

impl ServerController {
    pub fn new(rt: Handle) -> Self {
        ServerController {
            rt,
            status: ServerStatus::Stopped,
            shutdown: None,
            ws_shutdown: None,
            handles: Vec::new(),
        }
    }

    pub fn status(&self) -> &ServerStatus {
        &self.status
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, ServerStatus::Running { .. })
    }

    pub fn start(&mut self, state: AppState, plan: BindPlan) -> Result<Vec<SocketAddr>, String> {
        if self.is_running() {
            self.stop();
        }
        let mut listeners = Vec::new();
        for ip in &plan.addrs {
            let addr = SocketAddr::new(*ip, plan.port);
            match std::net::TcpListener::bind(addr) {
                Ok(l) => {
                    if let Err(e) = l.set_nonblocking(true) {
                        let msg = format!("{addr} 설정 실패: {e}");
                        self.status = ServerStatus::Failed(msg.clone());
                        return Err(msg);
                    }
                    listeners.push(l);
                }
                Err(e) => {
                    let msg = match e.kind() {
                        std::io::ErrorKind::AddrInUse => format!("포트 {} 사용 중", plan.port),
                        _ => format!("{addr} 바인딩 실패: {e}"),
                    };
                    self.status = ServerStatus::Failed(msg.clone());
                    return Err(msg);
                }
            }
        }
        if listeners.is_empty() {
            let msg = "바인딩할 주소가 없습니다".to_string();
            self.status = ServerStatus::Failed(msg.clone());
            return Err(msg);
        }

        let _ = state.shutdown.send(false);
        self.ws_shutdown = Some(state.shutdown.clone());
        let (tx, rx) = watch::channel(false);
        let app = build_router(state);
        let mut bound = Vec::new();
        for listener in listeners {
            let addr = listener.local_addr().map_err(|e| e.to_string())?;
            bound.push(addr);
            let app = app.clone();
            let mut rx = rx.clone();
            self.handles.push(self.rt.spawn(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                let _ = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    let _ = rx.wait_for(|stopping| *stopping).await;
                })
                .await;
            }));
        }
        self.shutdown = Some(tx);
        self.status = ServerStatus::Running { addrs: bound.clone() };
        Ok(bound)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.ws_shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(true);
        }
        let handles = std::mem::take(&mut self.handles);
        if !handles.is_empty() {
            let aborts: Vec<_> = handles.iter().map(|h| h.abort_handle()).collect();
            let all = futures::future::join_all(handles);
            let finished = self
                .rt
                .block_on(async { tokio::time::timeout(STOP_TIMEOUT, all).await.is_ok() });
            if !finished {
                for a in aborts {
                    a.abort();
                }
            }
        }
        self.status = ServerStatus::Stopped;
    }
}

impl Drop for ServerController {
    fn drop(&mut self) {
        if let Some(tx) = self.ws_shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(true);
        }
        for h in self.handles.drain(..) {
            h.abort();
        }
    }
}

/// 기본은 localhost + Tailscale 인터페이스. `lan` 이면 모든 인터페이스.
pub fn bind_plan(lan: bool, port: u16) -> BindPlan {
    let addrs = if lan {
        vec![IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)]
    } else {
        let mut v = vec![IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)];
        if let Some(ts) = crate::net::tailscale_ip() {
            v.push(IpAddr::V4(ts));
        }
        v
    };
    BindPlan { addrs, port }
}
