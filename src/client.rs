//! Blocking TCP client: one request, one response.

use crate::packet::{self, Command, Response};
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Client {
    addr: String,
    pin: u16,
    session: u32,
    timeout: Duration,
    stream: Option<TcpStream>,
}

impl Client {
    pub fn new(host: &str, port: u16, pin: u16) -> Self {
        // Upstream sends milliseconds since the epoch, truncated to 32 bits.
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Self {
            addr: format!("{host}:{port}"),
            pin,
            session: ms as u32,
            timeout: Duration::from_secs(5),
            stream: None,
        }
    }

    pub fn connect(&mut self) -> Result<()> {
        let addr = self
            .addr
            .to_socket_addrs()
            .with_context(|| format!("bad address {}", self.addr))?
            .next()
            .with_context(|| format!("no address for {}", self.addr))?;
        let s = TcpStream::connect_timeout(&addr, self.timeout)
            .with_context(|| format!("cannot connect to {addr} (is the printer on and are you on its Wi-Fi?)"))?;
        s.set_read_timeout(Some(self.timeout))?;
        s.set_write_timeout(Some(self.timeout))?;
        s.set_nodelay(true)?;
        self.stream = Some(s);
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(s) = self.stream.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    /// Send one command and read one response. Fault return codes become errors.
    pub fn send(&mut self, cmd: &Command) -> Result<Response> {
        let bytes = packet::encode_command(cmd, self.session, self.pin)?;
        let s = self.stream.as_mut().context("not connected")?;
        s.write_all(&bytes).context("send failed")?;

        let mut head = [0u8; 4];
        s.read_exact(&mut head).context("printer closed the connection or timed out")?;
        let len = u16::from_be_bytes([head[2], head[3]]) as usize;
        if len < 20 {
            bail!("printer sent a malformed length field ({len})");
        }
        let mut buf = vec![0u8; len];
        buf[..4].copy_from_slice(&head);
        s.read_exact(&mut buf[4..]).context("short response from printer")?;

        let resp = Response::parse(&buf)?;
        if let Some(why) = resp.error() {
            bail!("printer reported: {why} (code {})", resp.return_code);
        }
        Ok(resp)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}
