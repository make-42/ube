use crate::{
    search::{run_search, Seen},
    state::AppState,
    types::{ClientMsg, ServerMsg},
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinSet};

pub async fn ws_handler(ws: WebSocketUpgrade, State(app): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle(socket, app))
}

async fn handle(socket: WebSocket, app: Arc<AppState>) {
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMsg>(256);

    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            let Ok(text) = serde_json::to_string(&m) else { continue };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let mut tasks: JoinSet<()> = JoinSet::new(); // dropped with the connection => everything aborted
    let mut seen: Seen = Seen::default();

    while let Some(Ok(msg)) = stream.next().await {
        while tasks.try_join_next().is_some() {} // reap finished tasks
        match msg {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(text.as_str()) {
                Ok(ClientMsg::Reset) => {
                    tasks.abort_all();
                    seen = Seen::default();
                }
                Ok(ClientMsg::Search { epoch, q, kind, from, to }) => {
                    tasks.spawn(run_search(
                        app.clone(),
                        out_tx.clone(),
                        seen.clone(),
                        epoch,
                        q,
                        kind,
                        from,
                        to,
                    ));
                }
                Err(e) => tracing::debug!("bad client message: {e}"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    writer.abort();
}
