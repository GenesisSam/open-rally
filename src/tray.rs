//! macOS 메뉴바 앱. 메인 스레드에서 tao 이벤트 루프를 돌리고, 같은 프로세스의
//! tokio 런타임 위에서 서버를 켜고 끈다.

use std::net::Ipv4Addr;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{self, Config};
use crate::db::Db;
use crate::net::{lan_ip, tailscale_ip};
use crate::server::{bind_plan, AppState, ServerController, ServerStatus};

const ID_TOGGLE: &str = "toggle";
const ID_OPEN: &str = "open";
const ID_COPY: &str = "copy";
const ID_LAN: &str = "lan";
const ID_PASSWORD: &str = "password";
const ID_DATA: &str = "data";
const ID_QUIT: &str = "quit";
const ID_RETAIN_PREFIX: &str = "retain:";
/// 하위 메뉴에 보여줄 기간 선택지(일). 0 은 끔.
const RETENTION_CHOICES: &[u32] = &[0, 1, 7, 30, 90];

enum UserEvent {
    Menu(MenuId),
}

struct App {
    cfg: Config,
    ctl: ServerController,
    tray: Option<TrayIcon>,
    lan: bool,
    /// 실행 중인 서버의 상태. 자동 정리 기간을 바꿀 때 쓴다.
    state: Option<AppState>,
    /// config/DB 오류처럼 ServerController 밖에서 난 마지막 시작 실패 사유.
    last_error: Option<String>,
}

/// 메뉴 첫 줄. `.app` 에서는 stderr 가 안 보이므로 실패 사유를 여기 보여준다.
pub fn status_line(
    status: &ServerStatus,
    last_error: Option<&str>,
    port: u16,
    lan: bool,
    has_tailscale: bool,
) -> String {
    match status {
        ServerStatus::Running { .. } => {
            if !has_tailscale && !lan {
                format!("● 서버 실행 중 · 포트 {port} · Tailscale 없음, localhost 만")
            } else {
                format!("● 서버 실행 중 · 포트 {port}")
            }
        }
        ServerStatus::Failed(e) => format!("○ 시작 실패: {e}"),
        ServerStatus::Stopped => match last_error {
            Some(e) => format!("○ 시작 실패: {e}"),
            None => "○ 서버 꺼짐".to_string(),
        },
    }
}

/// 22×22 템플릿 아이콘. 켜짐은 채운 원, 꺼짐은 테두리만.
pub fn make_icon(filled: bool) -> Icon {
    const N: u32 = 22;
    let c = (N as f32 - 1.0) / 2.0;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let inside = if filled { d <= 7.0 } else { (5.4..=7.0).contains(&d) };
            let a = if inside { 255 } else { 0 };
            rgba.extend_from_slice(&[0, 0, 0, a]);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("icon")
}

fn url_for(ip: Ipv4Addr, port: u16) -> String {
    format!("http://{ip}:{port}")
}

impl App {
    fn running(&self) -> bool {
        self.ctl.is_running()
    }

    /// 메뉴에 보여줄 주소들: (Tailscale 주소, LAN 주소).
    fn urls(&self) -> (Option<String>, Option<String>) {
        if !self.running() {
            return (None, None);
        }
        let port = self.cfg.port;
        let ts = tailscale_ip().map(|ip| url_for(ip, port));
        let lan = if self.lan { lan_ip().map(|ip| url_for(ip, port)) } else { None };
        (ts, lan)
    }

    /// 열기·복사에 쓸 대표 주소.
    fn primary_url(&self) -> Option<String> {
        let (ts, lan) = self.urls();
        ts.or(lan)
            .or_else(|| self.running().then(|| url_for(Ipv4Addr::LOCALHOST, self.cfg.port)))
    }

    fn status_text(&self) -> String {
        status_line(
            self.ctl.status(),
            self.last_error.as_deref(),
            self.cfg.port,
            self.lan,
            tailscale_ip().is_some(),
        )
    }

    fn build_menu(&self) -> Menu {
        let menu = Menu::new();
        let running = self.running();
        let (ts, lan) = self.urls();
        let _ = menu.append(&MenuItem::new(self.status_text(), false, None));
        if let Some(u) = &ts {
            let _ = menu.append(&MenuItem::new(format!("   {u}"), false, None));
        }
        if let Some(u) = &lan {
            let _ = menu.append(&MenuItem::new(format!("   {u}  (LAN)"), false, None));
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id(
            ID_TOGGLE,
            if running { "서버 끄기" } else { "서버 켜기" },
            true,
            None,
        ));
        let _ = menu.append(&MenuItem::with_id(ID_OPEN, "브라우저에서 열기", running, None));
        let _ = menu.append(&MenuItem::with_id(ID_COPY, "접속 주소 복사", running, None));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&CheckMenuItem::with_id(ID_LAN, "LAN 에도 공개", true, self.lan, None));
        let retain = Submenu::new("첨부 자동 정리", true);
        for &days in RETENTION_CHOICES {
            let label = if days == 0 { "끄기".to_string() } else { format!("{days}일 지나면 삭제") };
            let _ = retain.append(&CheckMenuItem::with_id(
                format!("{ID_RETAIN_PREFIX}{days}"),
                label,
                true,
                self.cfg.retention_days == days,
                None,
            ));
        }
        let _ = menu.append(&retain);
        let _ = menu.append(&MenuItem::with_id(ID_PASSWORD, "비밀번호 파일 열기", true, None));
        let _ = menu.append(&MenuItem::with_id(ID_DATA, "데이터 폴더 열기", true, None));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id(ID_QUIT, "open-rally 종료", true, None));
        menu
    }

    fn refresh(&mut self) {
        let menu = self.build_menu();
        let running = self.running();
        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            let _ = tray.set_icon_templated(Some(make_icon(running)));
            let _ = tray.set_tooltip(Some(self.status_text()));
        }
    }

    /// 매번 config 를 다시 읽는다 (바꾼 비밀번호 반영).
    fn start_server(&mut self) {
        let result = config::load().and_then(|cfg| {
            let db = Db::open(&config::db_path(&cfg.data_dir))?;
            let state = AppState::new(db, config::uploads_dir(&cfg.data_dir), cfg.password.clone());
            state.retention.send_replace(cfg.retention_days);
            let plan = bind_plan(self.lan, cfg.port);
            self.cfg = cfg;
            self.state = Some(state.clone());
            self.ctl.start(state, plan).map(|_| ())
        });
        self.last_error = match result {
            Ok(()) => None,
            Err(e) => {
                eprintln!("open-rally: 서버 시작 실패: {e}");
                Some(e)
            }
        };
        self.refresh();
    }

    fn stop_server(&mut self) {
        self.ctl.stop();
        self.refresh();
    }

    fn set_retention(&mut self, days: u32) {
        match config::set_retention_days(&self.cfg.data_dir, days) {
            Ok(()) => {
                self.cfg.retention_days = days;
                if let Some(state) = &self.state {
                    state.retention.send_replace(days); // cleanup 작업이 바로 한 번 돈다
                }
            }
            Err(e) => eprintln!("open-rally: 자동 정리 설정 저장 실패: {e}"),
        }
        self.refresh();
    }

    fn on_menu(&mut self, id: &str, control_flow: &mut ControlFlow) {
        if let Some(days) = id.strip_prefix(ID_RETAIN_PREFIX).and_then(|d| d.parse::<u32>().ok()) {
            self.set_retention(days);
            return;
        }
        match id {
            ID_TOGGLE => {
                if self.running() {
                    self.stop_server();
                } else {
                    self.start_server();
                }
            }
            ID_OPEN => {
                if let Some(u) = self.primary_url() {
                    let _ = open::that(u);
                }
            }
            ID_COPY => {
                if let Some(u) = self.primary_url() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(u);
                    }
                }
            }
            ID_LAN => {
                self.lan = !self.lan;
                if self.running() {
                    self.ctl.stop();
                    self.start_server();
                } else {
                    self.refresh();
                }
            }
            ID_PASSWORD => {
                let _ = open::that(config::config_path(&self.cfg.data_dir));
            }
            ID_DATA => {
                let _ = open::that(&self.cfg.data_dir);
            }
            ID_QUIT => {
                self.ctl.stop();
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    }
}

pub fn run(cfg: Config) -> ! {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // Dock 아이콘 없이 메뉴바에만 보이게 한다 (.app 번들이 아니어도).
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(UserEvent::Menu(e.id));
    }));

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut app = App {
        cfg,
        ctl: ServerController::new(rt.handle().clone()),
        tray: None,
        lan: false,
        state: None,
        last_error: None,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                // macOS 에서는 트레이 아이콘을 이벤트 루프가 돈 뒤에 만들어야 한다.
                let tray = TrayIconBuilder::new()
                    .with_icon_templated(make_icon(false))
                    .with_tooltip("open-rally")
                    .with_menu(Box::new(app.build_menu()))
                    .build()
                    .expect("tray icon");
                app.tray = Some(tray);
                app.start_server();
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                let id = id.0.clone();
                app.on_menu(&id, control_flow);
            }
            _ => {}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_shows_config_or_db_errors() {
        let stopped = ServerStatus::Stopped;
        assert_eq!(status_line(&stopped, None, 7777, false, true), "○ 서버 꺼짐");
        assert_eq!(
            status_line(&stopped, Some("config.toml 형식이 올바르지 않습니다"), 7777, false, true),
            "○ 시작 실패: config.toml 형식이 올바르지 않습니다"
        );
        assert_eq!(
            status_line(&ServerStatus::Failed("포트 7777 사용 중".into()), None, 7777, false, true),
            "○ 시작 실패: 포트 7777 사용 중"
        );
        let running = ServerStatus::Running { addrs: vec![] };
        assert_eq!(status_line(&running, None, 7777, false, true), "● 서버 실행 중 · 포트 7777");
        assert_eq!(
            status_line(&running, None, 7777, false, false),
            "● 서버 실행 중 · 포트 7777 · Tailscale 없음, localhost 만"
        );
    }
}
