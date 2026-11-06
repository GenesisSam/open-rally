use tokio::sync::broadcast;

pub fn event_channel() -> broadcast::Sender<String> {
    broadcast::channel(256).0
}
