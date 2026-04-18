//! Standalone dartboard server binary.
//!
//! Same wire protocol as `dartboard --listen`, but without the ratatui /
//! crossterm / emoji-picker dependency footprint. Deploy this when the host
//! is headless (a box in the closet, a VPS, a CI runner) and you only want
//! the canvas + broadcast fan-out.

use std::net::SocketAddr;
use std::time::Duration;

use dartboard_server::{InMemStore, ServerHandle};

const HELP: &str = "\
dartboardd — headless dartboard server

USAGE:
  dartboardd <addr>               bind ws listener, serve until killed

EXAMPLES:
  dartboardd 127.0.0.1:8080
  dartboardd 0.0.0.0:8080         accept connections on every interface
  dartboardd 127.0.0.1:0          pick an OS-assigned port (printed on start)
";

fn main() -> std::io::Result<()> {
    let addr = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{}", msg);
            std::process::exit(if msg.starts_with("dartboardd") { 0 } else { 2 });
        }
    };

    let resolved = if addr.port() == 0 {
        let listener = std::net::TcpListener::bind(addr)?;
        let resolved = listener.local_addr()?;
        drop(listener);
        resolved
    } else {
        addr
    };

    let server = ServerHandle::spawn_local(InMemStore);
    server.bind_ws(resolved)?;
    eprintln!("dartboardd listening on ws://{}", resolved);
    eprintln!("press ctrl-c to stop");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn parse_args() -> Result<SocketAddr, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("-h" | "--help") => Err(HELP.to_string()),
        Some(addr) => addr.parse().map_err(|e| format!("bad addr: {}", e)),
        None => Err("dartboardd: missing <addr>. see --help.".to_string()),
    }
}
