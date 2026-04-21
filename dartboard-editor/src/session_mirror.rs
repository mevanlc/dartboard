use dartboard_core::{Canvas, CanvasOp, Peer, RgbColor, ServerMsg, UserId};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ConnectState {
    #[default]
    Pending,
    Welcomed,
    Rejected,
}

/// Host-facing view of a remote dartboard session. Tracks identity and peer
/// list as `ServerMsg`s arrive; emits typed `MirrorEvent`s so hosts can react
/// without re-matching the raw wire enum.
#[derive(Debug, Default, Clone)]
pub struct SessionMirror {
    pub peers: Vec<Peer>,
    pub my_user_id: Option<UserId>,
    pub my_color: Option<RgbColor>,
    pub connect_state: ConnectState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorEvent {
    Welcomed {
        my_user_id: UserId,
        my_color: RgbColor,
        peers: Vec<Peer>,
        snapshot: Canvas,
    },
    RemoteOp {
        op: CanvasOp,
        from: UserId,
    },
    PeerJoined(Peer),
    PeerLeft {
        user_id: UserId,
        index: usize,
    },
    ConnectRejected {
        reason: String,
    },
}

impl SessionMirror {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a server message and return the corresponding event for the host
    /// to react to. Self-echoed `OpBroadcast`s and unknown `PeerLeft`s return
    /// `None`; `Ack`/`Reject` are not surfaced.
    pub fn apply(&mut self, msg: ServerMsg) -> Option<MirrorEvent> {
        match msg {
            ServerMsg::Welcome {
                your_user_id,
                your_color,
                peers,
                snapshot,
            } => {
                self.my_user_id = Some(your_user_id);
                self.my_color = Some(your_color);
                self.peers = peers.clone();
                self.connect_state = ConnectState::Welcomed;
                Some(MirrorEvent::Welcomed {
                    my_user_id: your_user_id,
                    my_color: your_color,
                    peers,
                    snapshot,
                })
            }
            ServerMsg::OpBroadcast { op, from, .. } => {
                if Some(from) == self.my_user_id {
                    None
                } else {
                    Some(MirrorEvent::RemoteOp { op, from })
                }
            }
            ServerMsg::PeerJoined { peer } => {
                self.peers.push(peer.clone());
                Some(MirrorEvent::PeerJoined(peer))
            }
            ServerMsg::PeerLeft { user_id } => {
                let idx = self.peers.iter().position(|p| p.user_id == user_id)?;
                self.peers.remove(idx);
                Some(MirrorEvent::PeerLeft {
                    user_id,
                    index: idx,
                })
            }
            ServerMsg::ConnectRejected { reason } => {
                self.connect_state = ConnectState::Rejected;
                Some(MirrorEvent::ConnectRejected { reason })
            }
            ServerMsg::Ack { .. } | ServerMsg::Reject { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dartboard_core::{CanvasOp, Pos};

    fn make_peer(user_id: UserId, name: &str) -> Peer {
        Peer {
            user_id,
            name: name.to_string(),
            color: RgbColor::new(1, 2, 3),
        }
    }

    #[test]
    fn welcome_records_identity_and_peers() {
        let mut mirror = SessionMirror::new();
        assert_eq!(mirror.connect_state, ConnectState::Pending);

        let welcome_peers = vec![make_peer(2, "Bob")];
        let event = mirror.apply(ServerMsg::Welcome {
            your_user_id: 1,
            your_color: RgbColor::new(9, 9, 9),
            peers: welcome_peers.clone(),
            snapshot: Canvas::with_size(4, 2),
        });

        assert_eq!(mirror.my_user_id, Some(1));
        assert_eq!(mirror.my_color, Some(RgbColor::new(9, 9, 9)));
        assert_eq!(mirror.peers, welcome_peers);
        assert_eq!(mirror.connect_state, ConnectState::Welcomed);
        assert!(matches!(event, Some(MirrorEvent::Welcomed { .. })));
    }

    #[test]
    fn op_broadcast_from_self_is_swallowed() {
        let mut mirror = SessionMirror::new();
        mirror.my_user_id = Some(1);

        let op = CanvasOp::PaintCell {
            pos: Pos { x: 0, y: 0 },
            ch: 'x',
            fg: RgbColor::new(0, 0, 0),
        };

        assert!(mirror
            .apply(ServerMsg::OpBroadcast {
                from: 1,
                op: op.clone(),
                seq: 1,
            })
            .is_none());

        assert_eq!(
            mirror.apply(ServerMsg::OpBroadcast {
                from: 2,
                op: op.clone(),
                seq: 2,
            }),
            Some(MirrorEvent::RemoteOp { op, from: 2 })
        );
    }

    #[test]
    fn peer_join_and_leave_track_index() {
        let mut mirror = SessionMirror::new();
        mirror.apply(ServerMsg::PeerJoined {
            peer: make_peer(10, "a"),
        });
        mirror.apply(ServerMsg::PeerJoined {
            peer: make_peer(20, "b"),
        });

        assert_eq!(
            mirror.apply(ServerMsg::PeerLeft { user_id: 10 }),
            Some(MirrorEvent::PeerLeft {
                user_id: 10,
                index: 0,
            })
        );
        assert_eq!(mirror.peers.len(), 1);
        assert_eq!(mirror.peers[0].user_id, 20);

        // Unknown peer leave is a no-op.
        assert!(mirror.apply(ServerMsg::PeerLeft { user_id: 999 }).is_none());
    }

    #[test]
    fn connect_rejected_marks_state() {
        let mut mirror = SessionMirror::new();
        let event = mirror.apply(ServerMsg::ConnectRejected {
            reason: "server full".into(),
        });
        assert_eq!(mirror.connect_state, ConnectState::Rejected);
        assert!(matches!(event, Some(MirrorEvent::ConnectRejected { .. })));
    }
}
