//! TLS to a Chromecast through the mbedTLS in `libslotcast.so`, as the handheld has no TLS
//! library of its own. A Chromecast's certificate is its own, signed by nobody a client would
//! know, so it is not checked: the receiver was picked on the local network by its owner.

use std::ffi::{c_int, c_void};
use std::net::{SocketAddr, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::Duration;

use super::codec::Codecs;

/// A TLS connection over TCP.
pub struct Tls {
    lib: Arc<Codecs>,
    conn: *mut c_void,
    tcp: TcpStream,
}

// SAFETY: the connection is only ever used by the thread that has it.
unsafe impl Send for Tls {}

impl Tls {
    pub fn connect(lib: &Arc<Codecs>, addr: SocketAddr, timeout: Duration) -> Result<Tls, String> {
        let tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
        tcp.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        tcp.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        tcp.set_nodelay(true).map_err(|e| e.to_string())?;
        // SAFETY: a connected socket, which outlives the connection made over it.
        let conn = unsafe { (lib.tls_connect)(tcp.as_raw_fd()) };
        if conn.is_null() {
            return Err("the receiver would not talk TLS".into());
        }
        Ok(Tls {
            lib: lib.clone(),
            conn,
            tcp,
        })
    }

    /// Whether there is something to read within `wait`.
    pub fn readable(&self, wait: Duration) -> bool {
        // SAFETY: the connection is live.
        if unsafe { (self.lib.tls_pending)(self.conn) } > 0 {
            return true;
        }
        let mut fd = libc::pollfd {
            fd: self.tcp.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one pollfd, ours.
        unsafe { libc::poll(&mut fd, 1, wait.as_millis() as c_int) > 0 }
    }

    pub fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), String> {
        while !buf.is_empty() {
            // SAFETY: `buf` is that long and ours.
            let n = unsafe { (self.lib.tls_read)(self.conn, buf.as_mut_ptr(), buf.len() as c_int) };
            if n <= 0 {
                return Err("the receiver hung up".into());
            }
            buf = &mut buf[n as usize..];
        }
        Ok(())
    }

    pub fn write_all(&mut self, mut buf: &[u8]) -> Result<(), String> {
        while !buf.is_empty() {
            // SAFETY: `buf` is that long.
            let n = unsafe { (self.lib.tls_write)(self.conn, buf.as_ptr(), buf.len() as c_int) };
            if n <= 0 {
                return Err("the receiver hung up".into());
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }
}

impl Drop for Tls {
    fn drop(&mut self) {
        // SAFETY: made by this connection and let go once, before its socket closes.
        unsafe { (self.lib.tls_close)(self.conn) };
    }
}
