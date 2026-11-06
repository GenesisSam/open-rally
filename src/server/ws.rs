use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use tokio::sync::broadcast;

use super::AppState;

pub fn event_channel() -> broadcast::Sender<String> {
    broadcast::channel(256).0
}

pub async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle(socket, state))
}

/// 서버 → 클라이언트 이벤트만 보낸다. 클라이언트 메시지는 Close 만 처리한다.
async fn handle(mut socket: WebSocket, state: AppState) {
    let mut events = state.events.subscribe();
    let mut shutdown = state.shutdown.subscribe();
    if *shutdown.borrow_and_update() {
        return;
    }
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(text) => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    }
}
