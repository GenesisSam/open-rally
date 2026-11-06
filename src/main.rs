use open_rally::config;
use open_rally::db::Db;
use open_rally::server::{bind_plan, AppState, ServerController};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("open-rally [--serve [--lan]]\n  --serve  메뉴바 없이 서버만 전경에서 실행 (Ctrl+C 로 종료)\n  --lan    Tailscale 뿐 아니라 LAN 에도 공개");
        return;
    }
    if args.iter().any(|a| a == "--serve") {
        let lan = args.iter().any(|a| a == "--lan");
        if let Err(e) = serve(lan) {
            eprintln!("open-rally: {e}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(feature = "tray")]
    match config::load() {
        Ok(cfg) => open_rally::tray::run(cfg),
        Err(e) => {
            eprintln!("open-rally: {e}");
            std::process::exit(1);
        }
    }
    // headless 빌드(--no-default-features)에는 서버 모드뿐이다.
    #[cfg(not(feature = "tray"))]
    if let Err(e) = serve(args.iter().any(|a| a == "--lan")) {
        eprintln!("open-rally: {e}");
        std::process::exit(1);
    }
}

fn serve(lan: bool) -> Result<(), String> {
    let cfg = config::load()?;
    let db = Db::open(&config::db_path(&cfg.data_dir))?;
    let state = AppState::new(db, config::uploads_dir(&cfg.data_dir), cfg.password.clone());
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let mut ctl = ServerController::new(rt.handle().clone());
    let addrs = ctl.start(state, bind_plan(lan, cfg.port))?;
    for a in &addrs {
        println!("http://{a}");
    }
    if std::env::var("OPEN_RALLY_PASSWORD").map(|v| !v.is_empty()).unwrap_or(false) {
        println!("비밀번호: OPEN_RALLY_PASSWORD 환경변수");
    } else {
        println!("비밀번호 파일: {}", config::config_path(&cfg.data_dir).display());
    }
    println!("Ctrl+C 로 종료");
    let _ = rt.block_on(tokio::signal::ctrl_c());
    ctl.stop();
    Ok(())
}
