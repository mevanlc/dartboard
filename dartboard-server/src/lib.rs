//! In-process dartboard server + LocalClient.
//!
//! The server owns the canonical [`Canvas`], assigns globally monotonic
//! sequence numbers, and fans out [`ServerMsg`]s to connected clients. Each
//! [`LocalClient`] is a handle scoped to one session. For SP, the binary
//! creates one server and one LocalClient per local user; for MP over WS,
//! `Server::bind_ws` (added by the multiplayer plan) drives the same
//! per-client state machine from network sockets.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use dartboard_core::{
    Canvas, CanvasOp, Client, ClientMsg, ClientOpId, Peer, RgbColor, Seq, ServerMsg, UserId,
};

pub mod store;
mod ws;

pub use store::{CanvasStore, InMemStore};

/// Candidate colors offered to joining users whose requested color collides
/// with one already in use. Kept in sync with the client-side palette in
/// `dartboard/src/theme.rs`.
const PLAYER_PALETTE: [RgbColor; 8] = [
    RgbColor::new(255, 110, 64),
    RgbColor::new(255, 196, 64),
    RgbColor::new(145, 226, 88),
    RgbColor::new(72, 220, 170),
    RgbColor::new(84, 196, 255),
    RgbColor::new(128, 163, 255),
    RgbColor::new(192, 132, 255),
    RgbColor::new(255, 124, 196),
];

/// Max concurrent players on a single shared canvas. Equal to the palette
/// length so the seat cap preserves unique per-user colors.
pub const MAX_PLAYERS: usize = PLAYER_PALETTE.len();

fn resolve_user_color(requested: RgbColor, used: &[RgbColor]) -> RgbColor {
    if !used.contains(&requested) {
        return requested;
    }
    PLAYER_PALETTE
        .iter()
        .copied()
        .find(|c| !used.contains(c))
        .unwrap_or(requested)
}

/// A handle to the running server. Cloneable; every clone references the same
/// canonical canvas and client registry.
#[derive(Clone)]
pub struct ServerHandle {
    inner: Arc<ServerInner>,
}

struct ServerInner {
    state: Mutex<State>,
}

struct State {
    canvas: Canvas,
    seq: Seq,
    next_user_id: UserId,
    clients: Vec<ClientEntry>,
    store: Box<dyn CanvasStore>,
}

struct ClientEntry {
    peer: Peer,
    sender: EntrySender,
}

enum EntrySender {
    Local(mpsc::Sender<ServerMsg>),
    Ws(tokio::sync::mpsc::UnboundedSender<ServerMsg>),
}

impl EntrySender {
    fn send(&self, msg: ServerMsg) -> bool {
        match self {
            Self::Local(s) => s.send(msg).is_ok(),
            Self::Ws(s) => s.send(msg).is_ok(),
        }
    }
}

/// Introductory payload a client sends before any ops. Name + color are
/// echoed back to peers via PeerJoined.
#[derive(Debug, Clone)]
pub struct Hello {
    pub name: String,
    pub color: RgbColor,
}

/// Outcome of a connect attempt. Rejected connections leave no registered
/// state on the server.
pub enum ConnectOutcome {
    Accepted(LocalClient),
    Rejected(String),
}

impl ServerHandle {
    pub fn spawn_local<S: CanvasStore + 'static>(store: S) -> Self {
        let canvas = store.load().unwrap_or_default();
        let inner = Arc::new(ServerInner {
            state: Mutex::new(State {
                canvas,
                seq: 0,
                next_user_id: 1,
                clients: Vec::new(),
                store: Box::new(store),
            }),
        });
        Self { inner }
    }

    pub fn try_connect_local(&self, hello: Hello) -> ConnectOutcome {
        let (tx, rx) = mpsc::channel();
        match self.register(hello, EntrySender::Local(tx)) {
            Ok(user_id) => ConnectOutcome::Accepted(LocalClient {
                server: self.clone(),
                user_id,
                rx,
                next_client_op_id: 1,
            }),
            Err(reason) => ConnectOutcome::Rejected(reason),
        }
    }

    pub fn connect_local(&self, hello: Hello) -> LocalClient {
        match self.try_connect_local(hello) {
            ConnectOutcome::Accepted(client) => client,
            ConnectOutcome::Rejected(reason) => {
                panic!("connect_local rejected: {reason}")
            }
        }
    }

    /// Register a new client with an already-constructed sender. Used by the
    /// WS listener to hand a tokio mpsc sender in; [`connect_local`] is a thin
    /// wrapper for the std-mpsc case.
    pub(crate) fn register(&self, hello: Hello, sender: EntrySender) -> Result<UserId, String> {
        let mut state = self.inner.state.lock().unwrap();
        if state.clients.len() >= MAX_PLAYERS {
            let reason = format!(
                "dartboard is full ({} / {} players)",
                state.clients.len(),
                MAX_PLAYERS
            );
            let _ = sender.send(ServerMsg::ConnectRejected {
                reason: reason.clone(),
            });
            return Err(reason);
        }
        let user_id = state.next_user_id;
        state.next_user_id += 1;

        let used_colors: Vec<RgbColor> = state.clients.iter().map(|c| c.peer.color).collect();
        let color = resolve_user_color(hello.color, &used_colors);

        let peer = Peer {
            user_id,
            name: hello.name,
            color,
        };

        sender.send(ServerMsg::Welcome {
            your_user_id: user_id,
            your_color: color,
            peers: state.clients.iter().map(|c| c.peer.clone()).collect(),
            snapshot: state.canvas.clone(),
        });

        for entry in &state.clients {
            entry
                .sender
                .send(ServerMsg::PeerJoined { peer: peer.clone() });
        }

        state.clients.push(ClientEntry { peer, sender });
        Ok(user_id)
    }

    pub fn peer_count(&self) -> usize {
        self.inner.state.lock().unwrap().clients.len()
    }

    pub fn canvas_snapshot(&self) -> Canvas {
        self.inner.state.lock().unwrap().canvas.clone()
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

    pub(crate) fn submit_op(&self, user_id: UserId, client_op_id: ClientOpId, op: CanvasOp) {
        let mut state = self.inner.state.lock().unwrap();

        let State {
            canvas,
            seq,
            clients,
            store,
            ..
        } = &mut *state;

        canvas.apply(&op);
        *seq += 1;
        let seq = *seq;
        store.save(canvas);

        for entry in clients.iter() {
            if entry.peer.user_id == user_id {
                entry.sender.send(ServerMsg::Ack { client_op_id, seq });
            }
            entry.sender.send(ServerMsg::OpBroadcast {
                from: user_id,
                op: op.clone(),
                seq,
            });
        }
    }

    pub(crate) fn disconnect(&self, user_id: UserId) {
        let mut state = self.inner.state.lock().unwrap();
        state.clients.retain(|c| c.peer.user_id != user_id);
        for entry in &state.clients {
            entry.sender.send(ServerMsg::PeerLeft { user_id });
        }
    }
}

/// In-process client handle. Sends ops directly into the server under the
/// shared state lock; receives events over a std mpsc channel.
pub struct LocalClient {
    server: ServerHandle,
    user_id: UserId,
    rx: mpsc::Receiver<ServerMsg>,
    next_client_op_id: ClientOpId,
}

impl LocalClient {
    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn send(&mut self, msg: ClientMsg) -> Option<ClientOpId> {
        match msg {
            ClientMsg::Hello { .. } => None,
            ClientMsg::Op { op, .. } => Some(self.submit_op(op)),
        }
    }
}

impl Client for LocalClient {
    fn submit_op(&mut self, op: CanvasOp) -> ClientOpId {
        let id = self.next_client_op_id;
        self.next_client_op_id += 1;
        self.server.submit_op(self.user_id, id, op);
        id
    }

    fn try_recv(&mut self) -> Option<ServerMsg> {
        self.rx.try_recv().ok()
    }
}

impl Drop for LocalClient {
    fn drop(&mut self) {
        self.server.disconnect(self.user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dartboard_core::{ops::RowShift, Pos};

    fn red() -> RgbColor {
        RgbColor::new(255, 0, 0)
    }

    fn blue() -> RgbColor {
        RgbColor::new(0, 0, 255)
    }

    fn drain_events(client: &mut LocalClient) -> Vec<ServerMsg> {
        let mut events = Vec::new();
        while let Some(msg) = client.try_recv() {
            events.push(msg);
        }
        events
    }

    #[test]
    fn welcome_contains_snapshot_and_existing_peers() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut alice = server.connect_local(Hello {
            name: "alice".into(),
            color: red(),
        });
        let mut bob = server.connect_local(Hello {
            name: "bob".into(),
            color: blue(),
        });

        let alice_events = drain_events(&mut alice);
        let bob_events = drain_events(&mut bob);

        match &alice_events[0] {
            ServerMsg::Welcome { peers, .. } => assert!(peers.is_empty()),
            other => panic!("expected Welcome, got {:?}", other),
        }
        match &bob_events[0] {
            ServerMsg::Welcome { peers, .. } => {
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].name, "alice");
            }
            other => panic!("expected Welcome, got {:?}", other),
        }
        assert!(alice_events
            .iter()
            .any(|m| matches!(m, ServerMsg::PeerJoined { .. })));
    }

    #[test]
    fn submit_op_broadcasts_and_acks() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut alice = server.connect_local(Hello {
            name: "alice".into(),
            color: red(),
        });
        let mut bob = server.connect_local(Hello {
            name: "bob".into(),
            color: blue(),
        });
        let _ = drain_events(&mut alice);
        let _ = drain_events(&mut bob);

        alice.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 2, y: 1 },
            ch: 'A',
            fg: red(),
        });

        let alice_events = drain_events(&mut alice);
        let bob_events = drain_events(&mut bob);

        assert!(alice_events
            .iter()
            .any(|m| matches!(m, ServerMsg::Ack { .. })));
        assert!(alice_events
            .iter()
            .any(|m| matches!(m, ServerMsg::OpBroadcast { .. })));
        assert!(bob_events
            .iter()
            .any(|m| matches!(m, ServerMsg::OpBroadcast { .. })));

        let snap = server.canvas_snapshot();
        assert_eq!(snap.get(Pos { x: 2, y: 1 }), 'A');
    }

    #[test]
    fn sequence_numbers_are_monotonic() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut client = server.connect_local(Hello {
            name: "solo".into(),
            color: red(),
        });
        let _ = drain_events(&mut client);

        client.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 0, y: 0 },
            ch: 'A',
            fg: red(),
        });
        client.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 1, y: 0 },
            ch: 'B',
            fg: red(),
        });

        let mut seqs = Vec::new();
        for msg in drain_events(&mut client) {
            if let ServerMsg::OpBroadcast { seq, .. } = msg {
                seqs.push(seq);
            }
        }
        assert_eq!(seqs, vec![1, 2]);
    }

    #[test]
    fn shift_row_op_is_applied_server_side() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut client = server.connect_local(Hello {
            name: "solo".into(),
            color: red(),
        });
        let _ = drain_events(&mut client);

        client.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 0, y: 0 },
            ch: 'A',
            fg: red(),
        });
        client.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 1, y: 0 },
            ch: 'B',
            fg: red(),
        });
        client.submit_op(CanvasOp::ShiftRow {
            y: 0,
            kind: RowShift::PushLeft { to_x: 1 },
        });

        let snap = server.canvas_snapshot();
        assert_eq!(snap.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(snap.get(Pos { x: 1, y: 0 }), ' ');
    }

    #[test]
    fn colliding_color_gets_remapped_to_unused_palette_entry() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut alice = server.connect_local(Hello {
            name: "alice".into(),
            color: red(),
        });
        let mut bob = server.connect_local(Hello {
            name: "bob".into(),
            color: red(),
        });

        let alice_color = match drain_events(&mut alice).into_iter().next() {
            Some(ServerMsg::Welcome { your_color, .. }) => your_color,
            other => panic!("expected Welcome, got {:?}", other),
        };
        assert_eq!(alice_color, red());

        let bob_color = match drain_events(&mut bob).into_iter().next() {
            Some(ServerMsg::Welcome { your_color, .. }) => your_color,
            other => panic!("expected Welcome, got {:?}", other),
        };
        assert_ne!(
            bob_color,
            red(),
            "bob should not have kept the colliding color"
        );
        assert!(
            PLAYER_PALETTE.contains(&bob_color),
            "remapped color {:?} should come from the palette",
            bob_color
        );
    }

    #[test]
    fn dropping_client_broadcasts_peer_left() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut alice = server.connect_local(Hello {
            name: "alice".into(),
            color: red(),
        });
        let alice_id;
        {
            let bob = server.connect_local(Hello {
                name: "bob".into(),
                color: blue(),
            });
            alice_id = alice.user_id();
            drop(bob);
        }
        let events = drain_events(&mut alice);
        assert!(
            events
                .iter()
                .any(|m| matches!(m, ServerMsg::PeerLeft { .. })),
            "expected PeerLeft in {:?}",
            events
        );
        assert_eq!(server.peer_count(), 1);
        let _ = alice_id;
    }

    #[test]
    fn next_connect_is_rejected_when_server_is_full() {
        let server = ServerHandle::spawn_local(InMemStore);
        let mut clients = Vec::new();
        for (i, color) in PLAYER_PALETTE.iter().copied().enumerate().take(MAX_PLAYERS) {
            clients.push(server.connect_local(Hello {
                name: format!("peer{i}"),
                color,
            }));
        }

        match server.try_connect_local(Hello {
            name: "overflow".into(),
            color: red(),
        }) {
            ConnectOutcome::Rejected(reason) => {
                assert!(reason.to_lowercase().contains("full"), "reason: {reason}");
            }
            ConnectOutcome::Accepted(_) => panic!("server should be full"),
        }
        assert_eq!(server.peer_count(), MAX_PLAYERS);
    }
}
