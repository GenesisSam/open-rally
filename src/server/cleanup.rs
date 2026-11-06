//! 자동 정리: 기준 기간보다 오래된 글의 첨부 파일을 지운다. 글과 텍스트는 남는다.

use std::time::Duration;

use serde_json::json;

use super::AppState;

const DAY_MS: i64 = 86_400_000;
/// 정기 실행 간격.
pub const INTERVAL: Duration = Duration::from_secs(60 * 60);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 한 번 정리한다. 정리된 글 수를 돌려준다. retention 이 0 이면 아무것도 하지 않는다.
pub async fn sweep(state: &AppState, now_ms: i64) -> Result<usize, String> {
    let days = *state.retention.borrow();
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now_ms - i64::from(days) * DAY_MS;
    let posts = state.db.purge_attachments_before(cutoff)?;
    for post in &posts {
        let dir = state.uploads_dir.join(&post.id);
        for att in post.attachments.iter().filter(|a| a.purged) {
            if let Err(e) = tokio::fs::remove_file(dir.join(&att.filename)).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("open-rally: 정리 실패 {}/{}: {e}", post.id, att.filename);
                }
            }
        }
        let _ = tokio::fs::remove_dir(&dir).await; // 비어 있을 때만 지워진다
        let _ = state
            .events
            .send(json!({ "type": "updated", "post": post }).to_string());
    }
    Ok(posts.len())
}

/// 서버가 켜져 있는 동안 돈다: 시작 직후, 기간 설정이 바뀔 때, 그리고 매시간.
pub async fn run(state: AppState) {
    let mut shutdown = state.shutdown.subscribe();
    let mut retention = state.retention.subscribe();
    loop {
        if let Err(e) = sweep(&state, now_ms()).await {
            eprintln!("open-rally: 자동 정리 오류: {e}");
        }
        tokio::select! {
            _ = tokio::time::sleep(INTERVAL) => {}
            changed = retention.changed() => { if changed.is_err() { break; } }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() { break; }
            }
        }
    }
}
