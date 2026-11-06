# open-rally Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 개인 Mac 의 메뉴바에서 켜고 끄는, 비밀번호 하나로 들어가는 LAN/Tailscale 용 값·파일 게시판을 Rust 단일 바이너리로 만든다.

**Architecture:** 하나의 프로세스가 `tao` 이벤트 루프(메인 스레드, 메뉴바)와 `tokio` 런타임(서버)을 함께 띄운다. `ServerController` 가 런타임 위에서 `axum` 서버를 리스너 단위로 올리고 내린다. 저장은 `rusqlite` + 디스크 파일이고, 프론트엔드는 `include_str!` 로 바이너리에 넣은 정적 HTML/JS/CSS 다.

**Tech Stack:** Rust 2021, axum 0.8 (`multipart`, `ws`), tokio 1, tower-http (`fs`, `set-header`), axum-extra (`cookie`), rusqlite (`bundled`), ulid, serde/serde_json, toml, rand, subtle, if-addrs, dirs, mime_guess, tray-icon, muda, tao, open, arboard. 테스트: tower (`util`), tempfile, tokio-tungstenite(dev).

**Spec:** `docs/superpowers/specs/2026-11-06-open-rally-design.md`

## Global Constraints

- 언어는 Rust. 실행 파일 하나로 동작하고, 접속 PC 에는 아무것도 설치하지 않는다.
- 포트 기본값 `7777`, 환경변수 `OPEN_RALLY_PORT`. 데이터 폴더 기본값 `~/Library/Application Support/open-rally/`, 환경변수 `OPEN_RALLY_DATA_DIR`.
- 데이터 파일: `open-rally.db`, `uploads/<post_id>/<파일명>`, `config.toml` (`password = "..."`).
- 기본 바인딩은 Tailscale 인터페이스 IP(`100.64.0.0/10`) + `127.0.0.1`. "LAN 에도 공개" 가 켜지면 `0.0.0.0`. 이 설정은 저장하지 않는다.
- Host 허용: `localhost`, `127.0.0.1`, `[::1]`, `10/8`, `172.16/12`, `192.168/16`, `100.64/10`, `*.local`, `*.ts.net`. 거절은 `421`. `POST`/`DELETE`/`/ws` 에 `Origin` 이 있으면 같은 규칙으로 검사, 거절은 `403`.
- 세션 쿠키 `rally_session`: `HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000`. 토큰은 서버 메모리에만.
- 로그인 실패 5회 → 그 IP 10초 거절.
- 첨부 응답: `X-Content-Type-Options: nosniff`. `image/svg+xml`, `text/html`, `application/xhtml+xml` 은 항상 `attachment`. 그 외 `image/*`, `video/*` 는 `inline`, 나머지는 `attachment`. Range 지원.
- 업로드는 스트리밍으로 디스크에 쓴다. 크기 상한 없음.
- 파일명 정제: `/`, `\`, `..` 제거, 빈 이름은 `file`.
- CSP: `default-src 'self'; img-src 'self' blob:; media-src 'self'; connect-src 'self' ws: wss:`. 인라인 스크립트 금지.
- 글 목록: 최신순, 기본 100개, `before=<id>` 로 이전 페이지.
- 화면: 채팅형(입력창 아래, 최신 글 맨 아래). `Cmd/Ctrl+Enter` 전송, `Enter` 줄바꿈.
- 커밋은 작업마다. 커밋 메시지는 `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` 로 끝낸다.

## Review Focus

1. **파일명이 `../../x`, `a/b.txt`, 빈 문자열, 한글·공백 포함일 때** — 디스크 경로는 항상 `uploads/<post_id>/` 안이어야 하고 한글·공백은 그대로 보존돼야 한다. → Task 4 의 `sanitize_filename` 테스트.
2. **`Host: [::1]:7777`, `Host: 100.100.100.100:7777`, `Host: evil.com`** — IPv6 괄호와 포트를 벗겨낸 뒤 판정해야 한다. → Task 5 의 `host_allowed` 테스트.
3. **업로드가 중간에 끊겼을 때 (multipart 종료 경계 없음)** — DB 에 글이 남지 않고 `uploads/<post_id>/` 가 지워져야 한다. → Task 8 의 `create_post_cleans_up_on_truncated_body` 테스트.
4. **포트가 이미 사용 중일 때** — `ServerController::start` 가 `Err(문자열)` 을 돌려주고 패닉하지 않아야 한다. → Task 10 의 `start_fails_when_port_in_use` 테스트.
5. **클라이언트가 연결을 물고 있을 때 서버 끄기** — `stop()` 이 5초 안에 돌아와야 한다. → Task 10 의 `stop_returns_with_idle_connection_open` 테스트.

---

## File Structure

```
Cargo.toml
.gitignore
scripts/bundle-app.sh          .app 번들 생성 (LSUIElement)
assets/index.html              로그인 화면 + 게시판 (한 파일, JS 가 상태 전환)
assets/app.js
assets/style.css
src/main.rs                    진입점: 설정 로드, 런타임, --serve 플래그, 트레이 실행
src/config.rs                  데이터 폴더, 포트, config.toml 비밀번호
src/db.rs                      SQLite 스키마, Post/Attachment, 쿼리
src/files.rs                   파일명 정제, kind 판정, Content-Disposition 규칙
src/net.rs                     인터페이스 탐색(Tailscale/LAN IP), Host/Origin 허용 판정
src/auth.rs                    비밀번호 비교, SessionStore, RateLimiter, 쿠키
src/server/mod.rs              AppState, build_router, ServerController
src/server/middleware.rs       host/origin/session 미들웨어
src/server/routes.rs           /, 정적, /api/login, /api/session, /api/posts, /files
src/server/ws.rs               broadcast 채널, /ws 핸들러
src/tray.rs                    tao 루프, 메뉴, 아이콘, 액션
tests/common/mod.rs            테스트용 AppState + 로그인 헬퍼
tests/api.rs                   라우터 통합 테스트 (tower oneshot)
tests/server.rs                ServerController + WebSocket 실소켓 테스트
```

---

### Task 1: 프로젝트 뼈대

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `src/main.rs`
- Add to git: `docs/superpowers/specs/2026-11-06-open-rally-design.md`, `docs/superpowers/plans/2026-11-06-open-rally.md`

**Interfaces:**
- Produces: 크레이트 이름 `open-rally`, 바이너리 이름 `open-rally`. 이후 모든 Task 가 이 Cargo 구성을 쓴다.

- [ ] **Step 1: git 저장소 초기화**

Run: `cd /Users/satune/projects/open-rally && git init -b main`

- [ ] **Step 2: `.gitignore` 작성**

```
/target
/dist
/.superpowers
.DS_Store
```

- [ ] **Step 3: `Cargo.toml` 작성**

`[package] name = "open-rally", edition = "2021"`. 의존성은 Tech Stack 의 크레이트를 `cargo add` 로 넣는다 (`axum --features multipart,ws`, `tokio --features full`, `tower-http --features fs,set-header`, `axum-extra --features cookie`, `rusqlite --features bundled`, `serde --features derive`, `serde_json`, `ulid`, `toml`, `rand`, `subtle`, `if-addrs`, `dirs`, `mime_guess`, `tray-icon`, `muda`, `tao`, `open`, `arboard`, `tokio-util`, `futures`). dev: `tower --features util`, `tempfile`, `tokio-tungstenite`, `http-body-util`.

`[profile.release] strip = true, lto = "thin"`.

- [ ] **Step 4: `src/main.rs` 에 `fn main() { println!("open-rally"); }`**

- [ ] **Step 5: 빌드 확인**

Run: `cargo build`
Expected: 경고 없이 성공 (의존성 컴파일 시간은 길 수 있다).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore src/main.rs docs/
git commit -m "chore: scaffold open-rally crate with spec and plan"
```

---

### Task 2: 설정 (`config.rs`)

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (`mod config;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Config { pub data_dir: PathBuf, pub port: u16, pub password: String }
  pub fn load() -> Result<Config, String>;            // env 읽고, data_dir 생성, config.toml 읽거나 생성
  pub fn config_path(data_dir: &Path) -> PathBuf;     // data_dir/config.toml
  pub fn uploads_dir(data_dir: &Path) -> PathBuf;     // data_dir/uploads
  pub fn db_path(data_dir: &Path) -> PathBuf;         // data_dir/open-rally.db
  pub fn generate_password() -> String;               // 영숫자 12자
  ```
- 기본 `data_dir` 는 `dirs::data_dir().join("open-rally")` (macOS 에서 `~/Library/Application Support/open-rally`).

- [ ] **Step 1: 실패하는 테스트 작성 (`src/config.rs` 의 `#[cfg(test)]`)**

```rust
#[test]
fn load_creates_config_with_random_password_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    temp_env(&[("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap()), ("OPEN_RALLY_PORT", "8123")], || {
        let cfg = load().unwrap();
        assert_eq!(cfg.port, 8123);
        assert_eq!(cfg.password.len(), 12);
        assert!(config_path(dir.path()).exists());
        assert!(uploads_dir(dir.path()).is_dir());
        let again = load().unwrap();
        assert_eq!(again.password, cfg.password);
    });
}

#[test]
fn load_reads_existing_password() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "password = \"hunter2\"\n").unwrap();
    temp_env(&[("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap())], || {
        assert_eq!(load().unwrap().password, "hunter2");
        assert_eq!(load().unwrap().port, 7777);
    });
}
```

`temp_env` 는 env 를 설정하고 닫힐 때 되돌리는 테스트 헬퍼다. 테스트는 env 를 공유하므로 `#[serial]` 대신 `static LOCK: Mutex<()>` 로 직렬화한다.

- [ ] **Step 2: 실패 확인**

Run: `cargo test config::`
Expected: 컴파일 실패 (`load` 없음).

- [ ] **Step 3: `config.rs` 구현**

`config.toml` 파싱은 `toml::from_str::<ConfigFile>` (`#[derive(Deserialize)] struct ConfigFile { password: String }`). 없으면 `generate_password()` 로 만들어 `password = "..."` 한 줄로 쓴다. 포트 파싱 실패는 `Err`.

- [ ] **Step 4: 통과 확인**

Run: `cargo test config::`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: config loading with generated password"
```

---

### Task 3: 저장소 (`db.rs`)

**Files:**
- Create: `src/db.rs`
- Modify: `src/main.rs` (`mod db;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
  pub struct Attachment { pub id: String, pub filename: String, pub size: u64, pub mime: String, pub kind: String, pub url: String }
  #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
  pub struct Post { pub id: String, pub created_at: i64, pub body: Option<String>, pub attachments: Vec<Attachment> }
  pub struct NewAttachment { pub id: String, pub filename: String, pub size: u64, pub mime: String, pub kind: String }

  pub struct Db(Mutex<rusqlite::Connection>);
  impl Db {
      pub fn open(path: &Path) -> Result<Db, String>;     // 스키마 생성, PRAGMA foreign_keys=ON, journal_mode=WAL
      pub fn open_in_memory() -> Result<Db, String>;
      pub fn insert_post(&self, id: &str, created_at: i64, body: Option<&str>, atts: &[NewAttachment]) -> Result<Post, String>;
      pub fn list_posts(&self, before: Option<&str>, after: Option<&str>, limit: usize) -> Result<Vec<Post>, String>; // 최신순
      pub fn get_post(&self, id: &str) -> Result<Option<Post>, String>;
      pub fn get_attachment(&self, id: &str) -> Result<Option<(String /*post_id*/, Attachment)>, String>;
      pub fn delete_post(&self, id: &str) -> Result<bool, String>;  // 없으면 Ok(false)
  }
  ```
- `Attachment.url` 은 `format!("/files/{id}/{filename}")` (filename 은 percent-encoding 없이 그대로; 브라우저가 처리한다).
- 스키마는 설계 문서의 SQL 그대로.

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[test]
fn insert_and_list_newest_first() {
    let db = Db::open_in_memory().unwrap();
    db.insert_post("01A", 1, Some("first"), &[]).unwrap();
    db.insert_post("01B", 2, None, &[NewAttachment { id: "A1".into(), filename: "x.png".into(), size: 3, mime: "image/png".into(), kind: "image".into() }]).unwrap();
    let posts = db.list_posts(None, None, 100).unwrap();
    assert_eq!(posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["01B", "01A"]);
    assert_eq!(posts[0].attachments[0].url, "/files/A1/x.png");
    assert_eq!(db.list_posts(Some("01B"), None, 100).unwrap().len(), 1);
    assert_eq!(db.list_posts(None, Some("01A"), 100).unwrap()[0].id, "01B");
    assert_eq!(db.list_posts(None, None, 1).unwrap().len(), 1);
}

#[test]
fn delete_cascades_attachments() {
    let db = Db::open_in_memory().unwrap();
    db.insert_post("01A", 1, None, &[NewAttachment { id: "A1".into(), filename: "x".into(), size: 0, mime: "application/octet-stream".into(), kind: "file".into() }]).unwrap();
    assert!(db.delete_post("01A").unwrap());
    assert!(db.get_attachment("A1").unwrap().is_none());
    assert!(!db.delete_post("01A").unwrap());
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test db::` Expected: 컴파일 실패.

- [ ] **Step 3: `db.rs` 구현**

`list_posts` 는 `WHERE (?before IS NULL OR id < ?before) AND (?after IS NULL OR id > ?after) ORDER BY id DESC LIMIT ?`. 첨부는 post id 목록으로 한 번 더 조회해 붙인다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test db::` Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db.rs src/main.rs
git commit -m "feat: sqlite storage for posts and attachments"
```

---

### Task 4: 파일 규칙 (`files.rs`)

**Files:**
- Create: `src/files.rs`
- Modify: `src/main.rs` (`mod files;`)

**Interfaces:**
- Produces:
  ```rust
  pub fn sanitize_filename(raw: &str) -> String;
  pub fn kind_from_mime(mime: &str) -> &'static str;          // "image" | "video" | "file" (svg 는 "file")
  pub fn disposition(mime: &str, filename: &str) -> String;   // "inline" 또는 "attachment; filename=\"...\""
  pub fn unique_in_dir(dir: &Path, filename: &str) -> String; // 같은 이름이 있으면 "name-2.ext", "name-3.ext"
  pub fn guess_mime(browser_mime: Option<&str>, filename: &str) -> String; // 비었거나 octet-stream 이면 확장자로 추정
  ```

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[test]
fn sanitize_strips_path_parts_and_keeps_unicode() {
    assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
    assert_eq!(sanitize_filename("a/b\\c.txt"), "abc.txt");
    assert_eq!(sanitize_filename(""), "file");
    assert_eq!(sanitize_filename("..."), "file");
    assert_eq!(sanitize_filename("배포 도구 v2.dmg"), "배포 도구 v2.dmg");
}
#[test]
fn kind_and_disposition_rules() {
    assert_eq!(kind_from_mime("image/png"), "image");
    assert_eq!(kind_from_mime("image/svg+xml"), "file");
    assert_eq!(kind_from_mime("video/mp4"), "video");
    assert_eq!(kind_from_mime("application/x-mach-binary"), "file");
    assert_eq!(disposition("image/png", "a.png"), "inline");
    assert_eq!(disposition("video/quicktime", "a.mov"), "inline");
    assert_eq!(disposition("image/svg+xml", "a.svg"), "attachment; filename=\"a.svg\"");
    assert_eq!(disposition("text/html", "a.html"), "attachment; filename=\"a.html\"");
    assert_eq!(disposition("application/octet-stream", "deploy-tool"), "attachment; filename=\"deploy-tool\"");
}
#[test]
fn unique_in_dir_appends_counter() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.txt"), "").unwrap();
    std::fs::write(d.path().join("a-2.txt"), "").unwrap();
    assert_eq!(unique_in_dir(d.path(), "a.txt"), "a-3.txt");
    assert_eq!(unique_in_dir(d.path(), "b.txt"), "b.txt");
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test files::`

- [ ] **Step 3: 구현.** `disposition` 의 filename 은 `"` 와 제어문자를 제거한 뒤 넣는다. `guess_mime` 은 `mime_guess::from_path`.

- [ ] **Step 4: 통과 확인** — Run: `cargo test files::` Expected: 3 passed.

- [ ] **Step 5: Commit** — `git commit -m "feat: filename sanitising and attachment disposition rules"`

---

### Task 5: 네트워크 규칙 (`net.rs`)

**Files:**
- Create: `src/net.rs`
- Modify: `src/main.rs` (`mod net;`)

**Interfaces:**
- Produces:
  ```rust
  pub fn host_allowed(host_header: &str) -> bool;
  pub fn origin_allowed(origin_header: &str) -> bool;   // "scheme://host[:port]" 의 host 를 host_allowed 로
  pub fn tailscale_ip() -> Option<Ipv4Addr>;            // if-addrs, 100.64.0.0/10
  pub fn lan_ip() -> Option<Ipv4Addr>;                  // 10/8, 172.16/12, 192.168/16 중 첫 번째
  pub fn is_private_or_tailscale(ip: Ipv4Addr) -> bool;
  ```

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[test]
fn host_allowlist() {
    for ok in ["localhost", "localhost:7777", "127.0.0.1:7777", "[::1]:7777", "[::1]", "100.100.100.100:7777",
               "192.168.0.12", "10.0.0.5:80", "172.20.1.1", "mymac.local:7777", "mymac.tailnet.ts.net"] {
        assert!(host_allowed(ok), "{ok}");
    }
    for bad in ["evil.com", "evil.com:7777", "8.8.8.8", "172.32.0.1", "100.128.0.1", "ts.net.evil.com", "", "local"] {
        assert!(!host_allowed(bad), "{bad}");
    }
}
#[test]
fn origin_allowlist() {
    assert!(origin_allowed("http://100.100.100.100:7777"));
    assert!(origin_allowed("http://[::1]:7777"));
    assert!(!origin_allowed("http://evil.com"));
    assert!(!origin_allowed("null"));
}
#[test]
fn interface_lookup_does_not_panic() {
    let _ = tailscale_ip();
    let _ = lan_ip();
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test net::`

- [ ] **Step 3: 구현.** `host_allowed`: `[...]` 로 시작하면 닫는 괄호까지가 host, 아니면 마지막 `:` 앞까지(단 `:` 가 두 개 이상이면 전체를 IPv6 로 본다). IP 파싱되면 대역 검사, 아니면 소문자화 후 `== "localhost"` 또는 `.ends_with(".local")` 또는 `.ends_with(".ts.net")`.

- [ ] **Step 4: 통과 확인** — Run: `cargo test net::` Expected: 3 passed.

- [ ] **Step 5: Commit** — `git commit -m "feat: host/origin allowlist and interface discovery"`

---

### Task 6: 인증 요소 (`auth.rs`)

**Files:**
- Create: `src/auth.rs`
- Modify: `src/main.rs` (`mod auth;`)

**Interfaces:**
- Produces:
  ```rust
  pub const COOKIE_NAME: &str = "rally_session";
  pub const COOKIE_MAX_AGE_SECS: i64 = 2_592_000;
  pub fn password_matches(expected: &str, given: &str) -> bool;    // subtle 상수 시간 비교
  pub struct SessionStore(Mutex<HashSet<String>>);
  impl SessionStore { pub fn new() -> Self; pub fn issue(&self) -> String /* 32 바이트 hex */; pub fn is_valid(&self, token: &str) -> bool; }
  pub struct RateLimiter { max_failures: u32, block: Duration, .. }
  impl RateLimiter {
      pub fn new(max_failures: u32, block: Duration) -> Self;      // 서버에서는 (5, 10s)
      pub fn is_blocked(&self, ip: IpAddr, now: Instant) -> bool;
      pub fn record_failure(&self, ip: IpAddr, now: Instant);
      pub fn record_success(&self, ip: IpAddr);
  }
  pub fn session_cookie(token: &str) -> String; // "rally_session=<t>; HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000"
  ```

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[test]
fn rate_limiter_blocks_after_five_failures_for_ten_seconds() {
    let rl = RateLimiter::new(5, Duration::from_secs(10));
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let t0 = Instant::now();
    for _ in 0..4 { rl.record_failure(ip, t0); }
    assert!(!rl.is_blocked(ip, t0));
    rl.record_failure(ip, t0);
    assert!(rl.is_blocked(ip, t0));
    assert!(rl.is_blocked(ip, t0 + Duration::from_secs(9)));
    assert!(!rl.is_blocked(ip, t0 + Duration::from_secs(10)));
    rl.record_success(ip);
    assert!(!rl.is_blocked(ip, t0));
}
#[test]
fn session_tokens_are_unique_and_validated() {
    let s = SessionStore::new();
    let a = s.issue(); let b = s.issue();
    assert_ne!(a, b); assert_eq!(a.len(), 64);
    assert!(s.is_valid(&a)); assert!(!s.is_valid("nope"));
}
#[test]
fn password_compare() {
    assert!(password_matches("abc", "abc"));
    assert!(!password_matches("abc", "abd"));
    assert!(!password_matches("abc", "ab"));
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test auth::`

- [ ] **Step 3: 구현.** 차단이 풀리면(`now >= blocked_until`) 그 IP 의 실패 횟수를 0 으로 되돌린다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test auth::` Expected: 3 passed.

- [ ] **Step 5: Commit** — `git commit -m "feat: session store, rate limiter, password compare"`

---

### Task 7: 라우터 뼈대, 미들웨어, 로그인 (`server/mod.rs`, `server/middleware.rs`, `server/routes.rs` 일부)

**Files:**
- Create: `src/server/mod.rs`, `src/server/middleware.rs`, `src/server/routes.rs`, `src/server/ws.rs` (채널만), `assets/index.html` (임시 빈 뼈대), `assets/app.js` (빈 파일), `assets/style.css` (빈 파일), `tests/common/mod.rs`, `tests/api.rs`
- Modify: `src/main.rs` (`mod server;`, `pub` 로 라이브러리 노출 — 통합 테스트가 쓰도록 `src/lib.rs` 를 만들고 `main.rs` 는 `open_rally::...` 를 호출하는 구조로 바꾼다)

**Interfaces:**
- Produces:
  ```rust
  // server/mod.rs
  #[derive(Clone)]
  pub struct AppState(Arc<Inner>);
  pub struct Inner { pub db: Db, pub uploads_dir: PathBuf, pub password: String, pub sessions: SessionStore, pub limiter: RateLimiter, pub events: broadcast::Sender<String> }
  impl AppState { pub fn new(db: Db, uploads_dir: PathBuf, password: String) -> AppState; }
  impl std::ops::Deref for AppState { type Target = Inner; .. }   // 테스트와 핸들러가 state.events, state.db 로 접근
  pub fn build_router(state: AppState) -> Router;   // ConnectInfo<SocketAddr> 를 쓰므로 테스트에서는 요청 extension 으로 SocketAddr 를 넣는다
  // server/ws.rs
  pub fn event_channel() -> broadcast::Sender<String>;   // capacity 256
  ```
- 라우트 (이 Task 에서): `GET /` → `index.html` (CSP 헤더 포함), `GET /app.js`, `GET /style.css`, `POST /api/login`, `GET /api/session` (쿠키 유효하면 204, 아니면 401).
- 미들웨어 순서(바깥→안): Host 검사(421) → Origin 검사(403, `POST`/`DELETE` 와 `/ws` 만) → 세션 검사(401, 면제 경로: `/`, `/app.js`, `/style.css`, `/api/login`).
- `POST /api/login` 본문 `{"password":"..."}`. 차단 중이면 `429`, 틀리면 `401` + 실패 기록, 맞으면 `204` + `Set-Cookie`.
- `tests/common/mod.rs`:
  ```rust
  pub fn test_state() -> (AppState, tempfile::TempDir);           // in-memory db, password "pw"
  pub fn req(method: &str, uri: &str) -> http::request::Builder;  // Host: 127.0.0.1:7777, ConnectInfo 127.0.0.1:5000 삽입
  pub async fn login(app: &Router) -> String;                     // 로그인 후 "rally_session=..." 쿠키 값
  pub async fn body_json(resp: Response) -> serde_json::Value;
  ```

- [ ] **Step 1: 실패하는 통합 테스트 작성 (`tests/api.rs`)**

```rust
#[tokio::test]
async fn index_served_without_session_with_csp() {
    let (state, _d) = test_state(); let app = build_router(state);
    let r = app.oneshot(req("GET", "/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers()["content-security-policy"].to_str().unwrap().starts_with("default-src 'self'"));
}
#[tokio::test]
async fn bad_host_is_421_and_bad_origin_is_403() {
    let (state, _d) = test_state(); let app = build_router(state);
    let r = app.clone().oneshot(req("GET", "/").header("host", "evil.com").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), 421);
    let r = app.oneshot(req("POST", "/api/login").header("origin", "http://evil.com").header("content-type", "application/json").body(Body::from(r#"{"password":"pw"}"#)).unwrap()).await.unwrap();
    assert_eq!(r.status(), 403);
}
#[tokio::test]
async fn login_flow_and_rate_limit() {
    let (state, _d) = test_state(); let app = build_router(state);
    let r = app.clone().oneshot(req("GET", "/api/session").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), 401);
    for _ in 0..5 {
        let r = app.clone().oneshot(req("POST", "/api/login").header("content-type", "application/json").body(Body::from(r#"{"password":"wrong"}"#)).unwrap()).await.unwrap();
        assert_eq!(r.status(), 401);
    }
    let r = app.clone().oneshot(req("POST", "/api/login").header("content-type", "application/json").body(Body::from(r#"{"password":"pw"}"#)).unwrap()).await.unwrap();
    assert_eq!(r.status(), 429);
    // 다른 IP 는 통과
    let r = app.clone().oneshot(req_from("POST", "/api/login", "10.0.0.9:1").header("content-type", "application/json").body(Body::from(r#"{"password":"pw"}"#)).unwrap()).await.unwrap();
    assert_eq!(r.status(), 204);
    let cookie = r.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax") && cookie.contains("Max-Age=2592000"));
    let token = cookie.split(';').next().unwrap().to_string();
    let r = app.oneshot(req("GET", "/api/session").header("cookie", token).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), 204);
}
```

`req_from(method, uri, peer)` 는 `req` 와 같되 ConnectInfo 를 바꾼다. `common` 에 둔다.

- [ ] **Step 2: 실패 확인** — Run: `cargo test --test api` Expected: 컴파일 실패.

- [ ] **Step 3: `lib.rs` 로 모듈을 옮기고 `server/` 구현**

- Host 와 Origin 검사는 `axum::middleware::from_fn`. 세션 검사는 `from_fn_with_state`. 클라이언트 IP 는 `ConnectInfo<SocketAddr>` 를 요청 extension 에서 읽는다 (`req.extensions().get::<ConnectInfo<SocketAddr>>()`; 없으면 `127.0.0.1`).
- 정적 파일은 `include_str!("../../assets/...")`. `/` 응답에 CSP 헤더와 `Cache-Control: no-store`.
- `DefaultBodyLimit::disable()` 를 라우터에 건다 (Task 8 의 스트리밍 업로드를 위해).
- `assets/index.html` 은 이 Task 에서는 `<!doctype html><title>open-rally</title><script src="/app.js"></script>` 수준의 뼈대만.

- [ ] **Step 4: 통과 확인** — Run: `cargo test --test api` Expected: 3 passed. 기존 단위 테스트도 `cargo test` 로 전부 통과.

- [ ] **Step 5: Commit** — `git commit -m "feat: router with host/origin/session guards and login"`

---

### Task 8: 글 API (`server/routes.rs`)

**Files:**
- Modify: `src/server/routes.rs`, `src/server/mod.rs` (라우트 등록), `tests/api.rs`, `tests/common/mod.rs` (multipart 빌더 헬퍼)

**Interfaces:**
- Consumes: Task 3 `Db`, Task 4 `files::*`, Task 7 `AppState`, `events` 채널.
- Produces:
  - `GET /api/posts?before=&after=&limit=` → `{"posts":[Post...]}` 최신순. `limit` 기본 100, 최대 500.
  - `POST /api/posts` multipart. 필드 `body` (텍스트, 선택), `files` (0개 이상). → `201` + `Post` JSON. 둘 다 비면 `400`.
  - `DELETE /api/posts/{id}` → `204`, 없으면 `404`. `uploads/<id>/` 삭제.
  - 이벤트: 생성 시 `{"type":"created","post":Post}`, 삭제 시 `{"type":"deleted","id":"..."}` 를 `events` 로 보낸다.
  - `common::multipart(parts: &[(&str /*name*/, Option<&str> /*filename*/, &str /*mime*/, &[u8])]) -> (String /*content-type*/, Vec<u8>)`

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[tokio::test]
async fn posts_require_session() { /* GET /api/posts 쿠키 없이 → 401 */ }

#[tokio::test]
async fn create_text_post_list_and_delete() {
    // 로그인 → POST body="OPENAI_API_KEY=x\nB=2" 파일 없음 → 201, body 보존, attachments 비어 있음
    // GET /api/posts → 1개, 같은 id
    // DELETE → 204, 다시 DELETE → 404, GET → 0개
}

#[tokio::test]
async fn create_file_post_writes_to_disk_and_delete_removes_dir() {
    // files: ("deploy-tool", application/octet-stream, 1MB 의 의사난수 바이트), ("../x.png", image/png, b"png")
    // → 201. attachments[0].size == 1_048_576, attachments[1].filename == "x.png", kind == "image"
    // uploads/<post_id>/deploy-tool 의 바이트가 보낸 것과 동일
    // DELETE → uploads/<post_id> 가 없음
}

#[tokio::test]
async fn empty_post_is_400() { /* body "" 파일 없음 → 400 */ }

#[tokio::test]
async fn duplicate_filenames_in_one_post_get_suffix() {
    // files: a.txt, a.txt → filenames ["a.txt", "a-2.txt"]
}

#[tokio::test]
async fn create_post_cleans_up_on_truncated_body() {
    // multipart 본문에서 마지막 경계(--boundary--)를 잘라낸 채 전송 → 4xx 또는 5xx
    // GET /api/posts → 0개, uploads/ 아래 디렉토리 없음
}

#[tokio::test]
async fn created_and_deleted_events_are_broadcast() {
    // state.events.subscribe() 후 생성·삭제 → 두 메시지의 "type" 이 "created", "deleted"
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --test api`

- [ ] **Step 3: 구현**

- 생성 핸들러: `post_id = Ulid::new()`. 첫 파일 필드를 만나면 `uploads/<post_id>/` 생성. `field.chunk()` 루프로 `tokio::fs::File` 에 쓰며 `size` 누적. 어떤 단계든 실패하면 디렉토리를 지우고 에러 반환. 모든 필드를 읽은 뒤 텍스트도 파일도 없으면 디렉토리 지우고 `400`. DB insert 후 `events.send(...)` (수신자 없으면 에러를 무시).
- `body` 는 trim 하지 않고 그대로 저장하되, 전부 공백이면 없는 것으로 본다.
- 삭제: `db.delete_post` 가 true 면 `tokio::fs::remove_dir_all` (없어도 무시).

- [ ] **Step 4: 통과 확인** — Run: `cargo test --test api` Expected: 모두 통과.

- [ ] **Step 5: Commit** — `git commit -m "feat: post create/list/delete with streaming uploads"`

---

### Task 9: 첨부 파일 서빙 (`server/routes.rs`)

**Files:**
- Modify: `src/server/routes.rs`, `src/server/mod.rs`, `tests/api.rs`

**Interfaces:**
- Consumes: Task 3 `Db::get_attachment`, Task 4 `disposition`.
- Produces: `GET /files/{attachment_id}/{filename}` — 두 번째 세그먼트는 무시하고 DB 의 filename 으로 경로를 만든다. `tower_http::services::ServeFile::new(path).with_mime_type(mime)` 로 응답을 만든 뒤 `X-Content-Type-Options: nosniff`, `Content-Disposition` 을 덧붙인다. 없으면 `404`.

- [ ] **Step 1: 실패하는 테스트 작성**

```rust
#[tokio::test]
async fn file_headers_and_range() {
    // png 와 svg 와 "deploy-tool" 을 올린 뒤
    // GET /files/<png id>/x.png → 200, content-type image/png, content-disposition "inline", nosniff
    // GET /files/<svg id>/x.svg → content-disposition 이 attachment 로 시작
    // GET /files/<bin id>/deploy-tool → attachment; filename="deploy-tool"
    // GET + Range: bytes=0-3 → 206, content-range 가 "bytes 0-3/" 로 시작, 본문 4바이트
    // GET /files/nope/x → 404
    // 쿠키 없이 → 401
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --test api file_headers_and_range`

- [ ] **Step 3: 구현** (위 Interfaces 대로)

- [ ] **Step 4: 통과 확인** — Run: `cargo test --test api`

- [ ] **Step 5: Commit** — `git commit -m "feat: attachment download with range and safe disposition"`

---

### Task 10: WebSocket 과 ServerController (`server/ws.rs`, `server/mod.rs`, `tests/server.rs`)

**Files:**
- Modify: `src/server/ws.rs`, `src/server/mod.rs`
- Create: `tests/server.rs`

**Interfaces:**
- Produces:
  ```rust
  // ws.rs: GET /ws — 업그레이드 후 events.subscribe() 를 받아 각 메시지를 Text 로 보낸다. 클라이언트 메시지는 무시(Close 만 처리). Lagged 는 건너뛴다.
  // mod.rs
  pub struct BindPlan { pub addrs: Vec<IpAddr>, pub port: u16 }
  pub enum ServerStatus { Stopped, Running { addrs: Vec<SocketAddr> }, Failed(String) }
  pub struct ServerController { .. }
  impl ServerController {
      pub fn new(rt: tokio::runtime::Handle) -> Self;
      pub fn start(&mut self, state: AppState, plan: BindPlan) -> Result<Vec<SocketAddr>, String>; // 동기. 각 addr 에 std TcpListener bind (실패 시 전부 닫고 Err), tokio 로 변환, axum::serve(..).with_graceful_shutdown 를 spawn
      pub fn stop(&mut self);              // 동기. shutdown 신호 → 핸들을 최대 5초 기다림 → 안 끝나면 abort
      pub fn status(&self) -> &ServerStatus;
      pub fn is_running(&self) -> bool;
  }
  ```
- `axum::serve` 는 `app.into_make_service_with_connect_info::<SocketAddr>()` 로 (`ConnectInfo` 를 위해).

- [ ] **Step 1: 실패하는 테스트 작성 (`tests/server.rs`)**

```rust
#[test]
fn start_serves_and_ws_receives_events() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, _d) = test_state();
    let mut ctl = ServerController::new(rt.handle().clone());
    let addrs = ctl.start(state.clone(), BindPlan { addrs: vec!["127.0.0.1".parse().unwrap()], port: 0 }).unwrap();
    let port = addrs[0].port();
    rt.block_on(async {
        // reqwest 없이: tokio TcpStream 으로 raw HTTP 로그인 → 쿠키 추출
        // tokio_tungstenite::client_async 로 ws://127.0.0.1:{port}/ws (Host, Cookie 헤더 포함) 연결
        // state.events.send(r#"{"type":"created"}"#) → ws 에서 같은 텍스트 수신
    });
    ctl.stop();
    assert!(matches!(ctl.status(), ServerStatus::Stopped));
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn start_fails_when_port_in_use() {
    // std TcpListener 로 포트를 잡아둔 뒤 start → Err, status 는 Failed(_)
}

#[test]
fn stop_returns_with_idle_connection_open() {
    // start → std TcpStream::connect 로 연결만 하고 아무것도 안 보냄 → stop 이 5초 안에 반환
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --test server`

- [ ] **Step 3: 구현**

`stop` 의 대기는 `rt.block_on(tokio::time::timeout(5s, join_all(handles)))`. 유휴 keep-alive 연결 때문에 graceful 이 안 끝나면 abort 한다. 리스너는 serve 태스크가 끝나면 drop 된다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test --test server` Expected: 3 passed.

- [ ] **Step 5: Commit** — `git commit -m "feat: websocket events and start/stop server controller"`

---

### Task 11: 프론트엔드와 `--serve` 모드 (`assets/*`, `main.rs`)

**Files:**
- Modify: `assets/index.html`, `assets/app.js`, `assets/style.css`, `src/main.rs`

**Interfaces:**
- Consumes: Task 7–10 의 API 전부.
- Produces: `open-rally --serve [--lan]` — 트레이 없이 서버만 전경에서 띄운다 (`Ctrl+C` 로 종료). 개발·확인용이며 Task 12 의 트레이도 같은 `ServerController` 를 쓴다. 주소를 stdout 에 찍는다.

- [ ] **Step 1: `main.rs` 에 `--serve` 모드 구현**

`config::load()` → `Db::open` → `AppState::new` → `BindPlan { addrs: [127.0.0.1, tailscale_ip()?] (+ `--lan` 이면 `0.0.0.0` 하나로 대체), port }` → `ServerController::start` → `tokio::signal::ctrl_c` 대기 → `stop`.

- [ ] **Step 2: `index.html` / `style.css` / `app.js` 작성**

화면 구조 (설계 문서 "게시판 화면" 절과 목업 B):

- `#login` 오버레이: 비밀번호 input + 버튼. `POST /api/login`. 401 이면 "비밀번호가 틀렸습니다", 429 면 "잠시 후 다시 시도하세요". 성공하면 숨기고 `init()`.
- 상단 바: `open-rally`, `location.host`, 연결 점(`.dot.on` / `.dot.off`).
- `#list`: 글 카드. 시각은 오늘이면 `HH:MM`, 아니면 `MM/DD HH:MM`. 텍스트는 `<pre>` + **복사** 버튼 (`execCommand('copy')`; 실패 시 `<pre>` 전체 선택 + "Cmd+C 로 복사하세요" 토스트). 복사 성공 시 버튼 라벨 1.5초 "복사됨".
- 첨부: `kind=image` → `<img src=url>` (최대 높이 320px, 클릭 시 `window.open(url)`), `kind=video` → `<video controls preload="metadata" src=url>`; `error` 이벤트가 오면 그 자리에 파일 카드로 교체. `kind=file` → 아이콘 + 파일명 + 크기(KB/MB) + **다운로드** `<a href=url download>`.
- 각 카드 **삭제** → `confirm("삭제할까요?")` → `DELETE`.
- `#composer`: `<textarea>` (Enter 줄바꿈, `Cmd/Ctrl+Enter` 전송), 첨부 버튼(`<input type=file multiple>`), 드래그 앤 드롭(전체 창), 붙여넣기(`paste` 이벤트의 `clipboardData.files`). 선택된 파일은 전송 전 칩으로 보여주고 개별 제거 가능. 전송은 `XMLHttpRequest` 로 (`upload.onprogress` 로 진행률 바). 텍스트도 파일도 없으면 버튼 비활성.
- 로딩: 처음 `GET /api/posts?limit=100` 을 받아 역순으로 렌더(오래된 것 위). 맨 위로 스크롤하면 가장 오래된 id 로 `before=` 요청, 결과를 위에 끼우고 스크롤 위치 보존.
- WebSocket: `ws://${location.host}/ws`. `created` 면 끝에 추가, `deleted` 면 제거. 끊기면 1s→2s→4s…최대 30s 백오프로 재접속. 재접속 성공 시 목록을 다시 `limit=100` 으로 받아 전체 교체 (끊긴 동안의 삭제를 반영하기 위해).
- 자동 스크롤: 새 글이 올 때 스크롤이 바닥에서 80px 이내였으면 바닥으로.
- 스타일: 목업과 같은 분위기 (회색 배경, 흰 카드, 고정폭 텍스트 블록). 다크 모드는 `prefers-color-scheme` 으로 배경·카드 색만 바꾼다.
- 인라인 스크립트와 인라인 이벤트 속성은 쓰지 않는다 (CSP).

- [ ] **Step 3: 수동 확인**

Run: `OPEN_RALLY_DATA_DIR=/tmp/rally-dev cargo run -- --serve` 후 브라우저로 `http://127.0.0.1:7777`.
확인: 비밀번호(`/tmp/rally-dev/config.toml`) 입력 → 게시판. 여러 줄 텍스트 올리고 복사 버튼으로 복사. 이미지 붙여넣기, 파일 드롭, 100MB 이상 파일 업로드 시 진행률. 두 번째 탭에서 새 글이 새로고침 없이 보임. 삭제. 서버를 `Ctrl+C` 로 껐다 켜면 연결 점이 꺼졌다 켜지고 비밀번호를 다시 묻는다.

- [ ] **Step 4: `cargo test` 전체 통과 확인**

- [ ] **Step 5: Commit** — `git commit -m "feat: board frontend and --serve mode"`

---

### Task 12: 메뉴바 앱 (`tray.rs`, `main.rs`, `scripts/bundle-app.sh`)

**Files:**
- Create: `src/tray.rs`, `scripts/bundle-app.sh`
- Modify: `src/main.rs` (플래그 없이 실행하면 `tray::run(config)`)

**Interfaces:**
- Consumes: Task 10 `ServerController`, `BindPlan`; Task 5 `tailscale_ip`, `lan_ip`; Task 2 `config_path`.
- Produces: `pub fn run(config: Config) -> !` — 메인 스레드에서 `tao::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build()`. tokio `Runtime` 은 `run` 안에서 만들고 handle 만 컨트롤러에 넘긴다.

- [ ] **Step 1: `tray.rs` 구현**

- 메뉴 (설계 문서 "메뉴바 메뉴" 절 그대로): 상태 줄(비활성 `MenuItem`), 주소 줄 1~2개(비활성, 꺼져 있으면 숨김 대신 빈 텍스트), 구분선, 토글 항목, 브라우저에서 열기, 접속 주소 복사, 구분선, `CheckMenuItem` "LAN 에도 공개", 비밀번호 파일 열기, 데이터 폴더 열기, 구분선, 종료.
- 아이콘: `fn make_icon(filled: bool) -> tray_icon::Icon` — 22×22 RGBA 에 반지름 7 의 원을 그린다(채움/테두리 2px). `set_icon_as_template(true)`.
- **macOS 주의:** `TrayIcon` 은 `Event::NewEvents(StartCause::Init)` 안에서 만든다. 메뉴 이벤트는 `MenuEvent::set_event_handler` 에서 `EventLoopProxy::send_event(UserEvent::Menu(id))` 로 넘긴다. `ControlFlow::Wait`.
- 상태 갱신 `fn refresh(&mut self)`: 상태 줄 텍스트 `● 서버 실행 중 · 포트 7777` / `○ 서버 꺼짐` / `○ 시작 실패: <사유>` / `○ Tailscale 없음 · localhost 만` (Tailscale IP 가 없고 LAN 꺼짐일 때), 주소 줄, 토글 라벨, 열기·복사 활성 여부, 아이콘.
- 바인딩 계획 `fn bind_plan(lan: bool, port: u16) -> BindPlan`: `lan` 이면 `[0.0.0.0]`, 아니면 `[127.0.0.1] + tailscale_ip()`.
- 액션: 토글 → `start`/`stop`; LAN 체크 변경 → 실행 중이면 `stop` 후 새 계획으로 `start`; 열기 → `open::that(url)`; 복사 → `arboard::Clipboard::new()?.set_text(url)`; 비밀번호 파일 → `open::that(config_path)`; 데이터 폴더 → `open::that(data_dir)`; 종료 → `stop` 후 `ControlFlow::Exit`.
- 앱 시작 시 `start` 를 호출한다. 비밀번호는 매 `start` 때 `config::load()` 로 다시 읽어 `AppState` 를 새로 만든다 (바꾼 비밀번호 반영).
- 표시 URL: `http://<ip>:<port>` (Tailscale IP 우선, LAN 켜짐이면 `lan_ip()` 도).

- [ ] **Step 2: `scripts/bundle-app.sh` 작성**

`cargo build --release` → `dist/open-rally.app/Contents/MacOS/open-rally` 복사, `Contents/Info.plist` 에 `CFBundleName`, `CFBundleIdentifier` (`dev.satune.open-rally`), `CFBundleExecutable`, `CFBundlePackageType=APPL`, `LSUIElement=true`, `NSHighResolutionCapable=true`. 끝에 `open dist/open-rally.app` 안내 출력.

- [ ] **Step 3: 빌드와 수동 확인**

Run: `bash scripts/bundle-app.sh && open dist/open-rally.app`
확인: Dock 아이콘 없이 메뉴바 아이콘. 메뉴의 Tailscale 주소를 다른 PC(또는 iPhone 의 Tailscale) 브라우저로 열어 게시판 접속. "서버 끄기" 후 접속 불가, 아이콘 빈 원. "서버 켜기" 후 이전 글 유지. 같은 Wi-Fi 의 다른 기기에서 LAN IP 로는 열리지 않고, "LAN 에도 공개" 체크 후 열림. "접속 주소 복사" 로 클립보드에 URL. "비밀번호 파일 열기" 로 `config.toml` 이 열림. 종료.

- [ ] **Step 4: `cargo test` 전체 통과 확인**

- [ ] **Step 5: Commit** — `git commit -m "feat: menubar tray app with server toggle and app bundle script"`

---

### Task 13: 완료 기준 점검과 README

**Files:**
- Create: `README.md`

- [ ] **Step 1: 설계 문서 "완료 기준" 1~8 을 순서대로 수행하고 결과를 기록** (4번은 활성 모니터에서 `open-rally` 프로세스 메모리를 업로드 전후로 비교, 5번은 `shasum -a 256` 비교).

- [ ] **Step 2: `README.md` 작성** — 빌드(`bash scripts/bundle-app.sh`), 실행, 비밀번호 위치, 환경변수 2개, `--serve` 모드, 보안 전제(Tailscale 기본, LAN 토글) 를 각각 한두 줄로.

- [ ] **Step 3: 통과하지 못한 기준이 있으면 해당 Task 로 돌아가 고친 뒤 다시 확인**

- [ ] **Step 4: Commit** — `git commit -m "docs: README and acceptance check"`
