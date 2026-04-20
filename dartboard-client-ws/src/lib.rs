//! WebSocket [`Client`] implementation for dartboard.
//!
//! Runs a dedicated tokio runtime on its own OS thread. The runtime owns the
//! ws read/write halves and two bridging channels so the sync `Client` trait
//! (try_recv / submit_op) can talk to the async transport without forcing the
//! caller into tokio.

use std::sync::mpsc as stdmpsc;
use std::thread;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc as tkmpsc;
use tokio_tungstenite::tungstenite::Message;

use dartboard_core::{CanvasOp, Client, ClientMsg, ClientOpId, RgbColor, ServerMsg};

/// The same Hello shape [`dartboard_server::Hello`] uses; defined here to
/// avoid a server dep from the client-ws crate.
#[derive(Debug, Clone)]
pub struct Hello {
    pub name: String,
    pub color: RgbColor,
}

pub struct WebsocketClient {
    outbound: tkmpsc::UnboundedSender<ClientMsg>,
    inbound: stdmpsc::Receiver<ServerMsg>,
    next_client_op_id: ClientOpId,
    _runtime_thread: thread::JoinHandle<()>,
}

#[derive(Debug)]
pub enum ConnectError {
    Io(std::io::Error),
    // Boxed to keep ConnectError small; the underlying type is ~130 bytes.
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    Rejected(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {}", e),
            Self::Ws(e) => write!(f, "ws error: {}", e),
            Self::Rejected(reason) => write!(f, "{}", reason),
        }
    }
}

impl std::error::Error for ConnectError {}

impl From<std::io::Error> for ConnectError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for ConnectError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Ws(Box::new(e))
    }
}

impl WebsocketClient {
    pub fn connect(url: &str, hello: Hello) -> Result<Self, ConnectError> {
        let url = url.to_string();
        let (outbound_tx, outbound_rx) = tkmpsc::unbounded_channel::<ClientMsg>();
        let (inbound_tx, inbound_rx) = stdmpsc::channel::<ServerMsg>();
        let (ready_tx, ready_rx) = stdmpsc::channel();

        let runtime_thread = thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(ConnectError::Io(e)));
                    return;
                }
            };
            runtime.block_on(async move {
                match run_connection(url, hello, outbound_rx, inbound_tx, ready_tx).await {
                    Ok(()) => {}
                    Err(e) => eprintln!("ws client ended: {}", e),
                }
            });
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                outbound: outbound_tx,
                inbound: inbound_rx,
                next_client_op_id: 1,
                _runtime_thread: runtime_thread,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ConnectError::Io(std::io::Error::other(
                "ws thread disappeared",
            ))),
        }
    }
}

async fn run_connection(
    url: String,
    hello: Hello,
    mut outbound_rx: tkmpsc::UnboundedReceiver<ClientMsg>,
    inbound_tx: stdmpsc::Sender<ServerMsg>,
    ready_tx: stdmpsc::Sender<Result<(), ConnectError>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ws, _response) = match tokio_tungstenite::connect_async(&url).await {
        Ok(v) => v,
        Err(e) => {
            let _ = ready_tx.send(Err(ConnectError::Ws(Box::new(e))));
            return Ok(());
        }
    };
    let (mut write, mut read) = ws.split();

    let hello_text = serde_json::to_string(&ClientMsg::Hello {
        name: hello.name,
        color: hello.color,
    })?;
    write.send(Message::Text(hello_text)).await?;

    // Wait for the handshake response so a full server fails fast instead of
    // constructing a client that times out waiting for Welcome.
    let first_msg = match read.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str::<ServerMsg>(&text) {
            Ok(msg) => msg,
            Err(e) => {
                let _ = ready_tx.send(Err(ConnectError::Io(std::io::Error::other(format!(
                    "invalid server handshake: {e}"
                )))));
                return Ok(());
            }
        },
        Some(Ok(other)) => {
            let _ = ready_tx.send(Err(ConnectError::Io(std::io::Error::other(format!(
                "expected server handshake text frame, got {other:?}"
            )))));
            return Ok(());
        }
        Some(Err(e)) => {
            let _ = ready_tx.send(Err(ConnectError::Ws(Box::new(e))));
            return Ok(());
        }
        None => {
            let _ = ready_tx.send(Err(ConnectError::Io(std::io::Error::other(
                "server closed before handshake completed",
            ))));
            return Ok(());
        }
    };
    if let ServerMsg::ConnectRejected { reason } = first_msg {
        let _ = ready_tx.send(Err(ConnectError::Rejected(reason)));
        return Ok(());
    }
    if inbound_tx.send(first_msg).is_err() {
        let _ = ready_tx.send(Err(ConnectError::Io(std::io::Error::other(
            "client dropped before handshake completed",
        ))));
        return Ok(());
    }
    let _ = ready_tx.send(Ok(()));

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

    while let Some(frame) = read.next().await {
        let Ok(Message::Text(text)) = frame else {
            break;
        };
        let Ok(msg) = serde_json::from_str::<ServerMsg>(&text) else {
            continue;
        };
        if inbound_tx.send(msg).is_err() {
            break;
        }
    }

    writer.abort();
    Ok(())
}

impl Client for WebsocketClient {
    fn submit_op(&mut self, op: CanvasOp) -> ClientOpId {
        let id = self.next_client_op_id;
        self.next_client_op_id += 1;
        let _ = self.outbound.send(ClientMsg::Op {
            client_op_id: id,
            op,
        });
        id
    }

    fn try_recv(&mut self) -> Option<ServerMsg> {
        self.inbound.try_recv().ok()
    }
}
