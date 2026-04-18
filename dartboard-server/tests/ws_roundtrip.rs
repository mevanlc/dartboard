use std::time::Duration;

use dartboard_client_ws::{Hello, WebsocketClient};
use dartboard_core::{CanvasOp, Client, Pos, RgbColor, ServerMsg};
use dartboard_server::{InMemStore, ServerHandle};

/// Try to pick a loopback port that isn't currently bound. Not perfect
/// (there's a small race with the actual bind), but good enough for CI where
/// nothing else is listening on high ports.
fn pick_addr() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

fn drain<C: Client>(client: &mut C) -> Vec<ServerMsg> {
    let mut out = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Some(msg) = client.try_recv() {
            out.push(msg);
        } else if !out.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
            if client.try_recv().is_none() {
                break;
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    out
}

fn wait_for<C: Client, F: Fn(&ServerMsg) -> bool>(
    client: &mut C,
    pred: F,
    timeout: Duration,
) -> Option<ServerMsg> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(msg) = client.try_recv() {
            if pred(&msg) {
                return Some(msg);
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    None
}

#[test]
fn ws_two_clients_see_each_others_paints() {
    let server = ServerHandle::spawn_local(InMemStore);
    let addr = pick_addr();
    server.bind_ws(addr).expect("bind_ws should succeed");
    let url = format!("ws://{}", addr);

    let mut alice = WebsocketClient::connect(
        &url,
        Hello {
            name: "alice".into(),
            color: RgbColor::new(255, 0, 0),
        },
    )
    .expect("alice connect");

    let mut bob = WebsocketClient::connect(
        &url,
        Hello {
            name: "bob".into(),
            color: RgbColor::new(0, 0, 255),
        },
    )
    .expect("bob connect");

    // Consume welcomes and peer-join events
    let _ = drain(&mut alice);
    let _ = drain(&mut bob);

    alice.submit_op(CanvasOp::PaintCell {
        pos: Pos { x: 0, y: 0 },
        ch: 'X',
        fg: RgbColor::new(255, 0, 0),
    });

    let seen = wait_for(
        &mut bob,
        |m| {
            matches!(
                m,
                ServerMsg::OpBroadcast {
                    op: CanvasOp::PaintCell { ch: 'X', .. },
                    ..
                }
            )
        },
        Duration::from_secs(2),
    );
    assert!(seen.is_some(), "bob should see alice's OpBroadcast");

    let snap = server.canvas_snapshot();
    assert_eq!(snap.get(Pos { x: 0, y: 0 }), 'X');
}

#[test]
fn ws_concurrent_disjoint_paints_both_land() {
    let server = ServerHandle::spawn_local(InMemStore);
    let addr = pick_addr();
    server.bind_ws(addr).expect("bind_ws should succeed");
    let url = format!("ws://{}", addr);

    let mut alice = WebsocketClient::connect(
        &url,
        Hello {
            name: "alice".into(),
            color: RgbColor::new(255, 0, 0),
        },
    )
    .expect("alice connect");
    let mut bob = WebsocketClient::connect(
        &url,
        Hello {
            name: "bob".into(),
            color: RgbColor::new(0, 0, 255),
        },
    )
    .expect("bob connect");
    let _ = drain(&mut alice);
    let _ = drain(&mut bob);

    alice.submit_op(CanvasOp::PaintCell {
        pos: Pos { x: 0, y: 0 },
        ch: 'A',
        fg: RgbColor::new(255, 0, 0),
    });
    bob.submit_op(CanvasOp::PaintCell {
        pos: Pos { x: 1, y: 0 },
        ch: 'B',
        fg: RgbColor::new(0, 0, 255),
    });

    std::thread::sleep(Duration::from_millis(200));
    let snap = server.canvas_snapshot();
    assert_eq!(snap.get(Pos { x: 0, y: 0 }), 'A');
    assert_eq!(snap.get(Pos { x: 1, y: 0 }), 'B');
}
