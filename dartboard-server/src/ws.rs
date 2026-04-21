//! WebSocket listener that bridges ws frames to the sync Server state.
//!
//! The Server's canonical state is guarded by a Mutex and every mutation is
//! sync. The ws bits live on a tokio runtime — spawned by `bind_ws` on a
//! dedicated thread so the sync Server API is unchanged.

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

use dartboard_core::{ClientMsg, ServerMsg};
use dartboard_local::ServerSink;

use crate::{Hello, ServerHandle};

struct WsSink(tokio::sync::mpsc::UnboundedSender<ServerMsg>);

impl ServerSink for WsSink {
    fn send(&self, msg: ServerMsg) -> bool {
        self.0.send(msg).is_ok()
    }
}

pub(crate) async fn accept_and_run(
    server: ServerHandle,
    stream: TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws.split();

    let hello = match read.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientMsg>(&text)? {
            ClientMsg::Hello { name, color } => Hello { name, color },
            other => return Err(format!("expected Hello, got {:?}", other).into()),
        },
        other => return Err(format!("expected Hello frame, got {:?}", other).into()),
    };

    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else {
                break;
            };
            if write.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });
    let user_id = match server.register_transport(hello, Box::new(WsSink(outbound_tx))) {
        Ok(user_id) => user_id,
        Err(_) => {
            let _ = writer.await;
            return Ok(());
        }
    };

    while let Some(frame) = read.next().await {
        let Ok(Message::Text(text)) = frame else {
            break;
        };
        let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else {
            continue;
        };
        match msg {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Op { client_op_id, op } => {
                server.submit_op_for(user_id, client_op_id, op);
            }
        }
    }

    server.disconnect_user(user_id);
    writer.abort();
    Ok(())
}
