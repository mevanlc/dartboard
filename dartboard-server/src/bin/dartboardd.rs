//! Standalone dartboard server binary.
//!
//! Same wire protocol as `dartboard --listen`, but without the ratatui /
//! crossterm / emoji-picker dependency footprint. Deploy this when the host
//! is headless (a box in the closet, a VPS, a CI runner) and you only want
//! the canvas + broadcast fan-out.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use dartboard_server::{InMemStore, ServerHandle};

const DEFAULT_PORT: u16 = 9199;
const DEFAULT_ADDR: &str = "127.0.0.1:9199";

const HELP: &str = "\
dartboardd — headless dartboard server

USAGE:
  dartboardd [addr]               bind ws listener, serve until killed
                                  default: 127.0.0.1:9199
                                  if port omitted, 9199 is used

EXAMPLES:
  dartboardd                      bind 127.0.0.1:9199
  dartboardd 127.0.0.1:9199
  dartboardd 0.0.0.0              accept connections on every interface (port 9199)
  dartboardd 0.0.0.0:9199
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
        Some(addr) => parse_addr(addr),
        None => parse_addr(DEFAULT_ADDR),
    }
}

fn parse_addr(s: &str) -> Result<SocketAddr, String> {
    if let Ok(sa) = s.parse::<SocketAddr>() {
        return Ok(sa);
    }
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_PORT));
    }
    Err(format!("bad addr: {}", s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_socket_addr_is_preserved() {
        assert_eq!(
            parse_addr("10.0.0.1:7000").unwrap(),
            "10.0.0.1:7000".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn bare_ipv4_gets_default_port() {
        assert_eq!(
            parse_addr("0.0.0.0").unwrap(),
            "0.0.0.0:9199".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn bare_ipv6_gets_default_port() {
        assert_eq!(
            parse_addr("::1").unwrap(),
            "[::1]:9199".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn default_addr_parses() {
        assert_eq!(
            parse_addr(DEFAULT_ADDR).unwrap(),
            "127.0.0.1:9199".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_addr("not-an-address").is_err());
    }
}
