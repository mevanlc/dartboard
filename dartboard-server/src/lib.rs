//! WebSocket transport wrapper for [`dartboard_local`].
//!
//! Consumers that only need the in-process server and local client can depend
//! on `dartboard-local` directly. This crate keeps the websocket listener and
//! headless `dartboardd` binary while preserving the existing `ServerHandle`
//! convenience surface for ws-hosting callers.

use dartboard_core::{Canvas, CanvasOp, ClientOpId, UserId};

pub use dartboard_local::{
    CanvasStore, ConnectOutcome, Hello, InMemStore, LocalClient, MAX_PLAYERS,
};

mod ws;

#[derive(Clone)]
pub struct ServerHandle {
    local: dartboard_local::ServerHandle,
}

impl ServerHandle {
    pub fn spawn_local<S: CanvasStore + 'static>(store: S) -> Self {
        Self {
            local: dartboard_local::ServerHandle::spawn_local(store),
        }
    }

    pub fn try_connect_local(&self, hello: Hello) -> ConnectOutcome {
        self.local.try_connect_local(hello)
    }

    pub fn connect_local(&self, hello: Hello) -> LocalClient {
        self.local.connect_local(hello)
    }

    pub fn peer_count(&self) -> usize {
        self.local.peer_count()
    }

    pub fn canvas_snapshot(&self) -> Canvas {
        self.local.canvas_snapshot()
    }

    /// Bind a TCP listener on `addr`, spawn a dedicated tokio runtime thread,
    /// and accept WebSocket connections. Each accepted connection talks the
    /// same [`ClientMsg`]/[`ServerMsg`] protocol as [`LocalClient`], framed as
    /// JSON over ws frames.
    ///
    /// Blocks only for the initial bind; returns once the listener is live.
    /// The accept loop runs until the process exits.
    pub fn bind_ws(&self, addr: std::net::SocketAddr) -> std::io::Result<()> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server = self.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            runtime.block_on(async move {
                match tokio::net::TcpListener::bind(addr).await {
                    Ok(listener) => {
                        let _ = ready_tx.send(Ok(()));
                        loop {
                            let Ok((stream, _)) = listener.accept().await else {
                                break;
                            };
                            let server = server.clone();
                            tokio::spawn(async move {
                                if let Err(e) = ws::accept_and_run(server, stream).await {
                                    eprintln!("ws connection ended: {}", e);
                                }
                            });
                        }
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        });

        ready_rx
            .recv()
            .unwrap_or_else(|_| Err(std::io::Error::other("ws thread disappeared")))
    }

    pub(crate) fn register_transport(
        &self,
        hello: Hello,
        sender: Box<dyn dartboard_local::ServerSink>,
    ) -> Result<UserId, String> {
        self.local.register_transport(hello, sender)
    }

    pub(crate) fn submit_op_for(&self, user_id: UserId, client_op_id: ClientOpId, op: CanvasOp) {
        self.local.submit_op_for(user_id, client_op_id, op);
    }
    pub(crate) fn disconnect_user(&self, user_id: UserId) {
        self.local.disconnect_user(user_id);
    }
}
