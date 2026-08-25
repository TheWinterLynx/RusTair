use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::BackendSerialPort;

const SOCKET_CHUNK: usize = 4096;
const QUEUE_LIMIT: usize = 64 * 1024;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(crate) enum SimhSerialBridgeError {
    Io(std::io::Error),
    QueueFull { port: BackendSerialPort },
    Disconnected { port: BackendSerialPort },
    ConnectTimeout { port0: bool, port1: bool },
}

impl fmt::Display for SimhSerialBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::QueueFull { port } => write!(f, "SIMH serial queue is full for {port:?}"),
            Self::Disconnected { port } => write!(f, "SIMH serial socket disconnected for {port:?}"),
            Self::ConnectTimeout { port0, port1 } => write!(
                f,
                "timed out waiting for SIMH M2SIO raw TCP connection(s): port0_connected={port0}, port1_connected={port1}"
            ),
        }
    }
}

impl std::error::Error for SimhSerialBridgeError {}

impl From<std::io::Error> for SimhSerialBridgeError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}

struct SimhRawTcpPort {
    logical_port: BackendSerialPort,
    listener: TcpListener,
    stream: Option<TcpStream>,
    to_simh: VecDeque<u8>,
    from_simh: VecDeque<u8>,
}

impl SimhRawTcpPort {
    fn bind(logical_port: BackendSerialPort) -> Result<Self, SimhSerialBridgeError> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            logical_port,
            listener,
            stream: None,
            to_simh: VecDeque::new(),
            from_simh: VecDeque::new(),
        })
    }

    fn listen_port(&self) -> u16 {
        self.listener
            .local_addr()
            .expect("bound SIMH serial listener must have a local address")
            .port()
    }

    fn connected(&self) -> bool { self.stream.is_some() }

    fn accept_pending(&mut self) -> Result<(), SimhSerialBridgeError> {
        if self.stream.is_some() {
            return Ok(());
        }
        match self.listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
                stream.set_nonblocking(true)?;
                let _ = stream.set_nodelay(true);
                self.stream = Some(stream);
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn queue_to_simh(&mut self, byte: u8) -> Result<(), SimhSerialBridgeError> {
        if self.to_simh.len() >= QUEUE_LIMIT {
            return Err(SimhSerialBridgeError::QueueFull { port: self.logical_port });
        }
        self.to_simh.push_back(byte);
        Ok(())
    }

    fn poll(&mut self) -> Result<(), SimhSerialBridgeError> {
        self.accept_pending()?;
        if self.stream.is_none() {
            return Err(SimhSerialBridgeError::Disconnected { port: self.logical_port });
        }
        self.flush_to_simh()?;
        self.read_from_simh()?;
        Ok(())
    }

    fn flush_to_simh(&mut self) -> Result<(), SimhSerialBridgeError> {
        while !self.to_simh.is_empty() {
            let chunk: Vec<u8> = self.to_simh.iter().take(SOCKET_CHUNK).copied().collect();
            let result = self
                .stream
                .as_mut()
                .expect("connected SIMH serial stream")
                .write(&chunk);
            match result {
                Ok(0) => return self.disconnect_error(),
                Ok(count) => {
                    for _ in 0..count {
                        self.to_simh.pop_front();
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => return self.disconnect_error(),
            }
        }
        Ok(())
    }

    fn read_from_simh(&mut self) -> Result<(), SimhSerialBridgeError> {
        loop {
            let room = QUEUE_LIMIT.saturating_sub(self.from_simh.len());
            if room == 0 {
                return Ok(());
            }
            let mut buffer = [0u8; SOCKET_CHUNK];
            let max_read = room.min(buffer.len());
            let result = self
                .stream
                .as_mut()
                .expect("connected SIMH serial stream")
                .read(&mut buffer[..max_read]);
            match result {
                Ok(0) => return self.disconnect_error(),
                Ok(count) => self.from_simh.extend(&buffer[..count]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(_) => return self.disconnect_error(),
            }
        }
    }

    fn disconnect_error<T>(&mut self) -> Result<T, SimhSerialBridgeError> {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        Err(SimhSerialBridgeError::Disconnected { port: self.logical_port })
    }

    fn disconnect(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.to_simh.clear();
        self.from_simh.clear();
    }

    fn clear_queues(&mut self) {
        self.to_simh.clear();
        self.from_simh.clear();
    }
}

pub(crate) struct SimhM2SioBridge {
    port0: SimhRawTcpPort,
    port1: SimhRawTcpPort,
}

impl SimhM2SioBridge {
    pub(crate) fn bind_loopback() -> Result<Self, SimhSerialBridgeError> {
        Ok(Self {
            port0: SimhRawTcpPort::bind(BackendSerialPort::Port0)?,
            port1: SimhRawTcpPort::bind(BackendSerialPort::Port1)?,
        })
    }

    pub(crate) fn listen_ports(&self) -> (u16, u16) {
        (self.port0.listen_port(), self.port1.listen_port())
    }

    pub(crate) fn wait_for_connections(
        &mut self,
        timeout: Duration,
    ) -> Result<(), SimhSerialBridgeError> {
        let deadline = Instant::now() + timeout;
        loop {
            self.port0.accept_pending()?;
            self.port1.accept_pending()?;
            if self.port0.connected() && self.port1.connected() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(SimhSerialBridgeError::ConnectTimeout {
                    port0: self.port0.connected(),
                    port1: self.port1.connected(),
                });
            }
            thread::sleep(CONNECT_POLL_INTERVAL);
        }
    }

    pub(crate) fn connected(&self, port: BackendSerialPort) -> bool {
        self.port(port).connected()
    }

    pub(crate) fn poll(&mut self) -> Result<(), SimhSerialBridgeError> {
        self.port0.poll()?;
        self.port1.poll()?;
        Ok(())
    }

    pub(crate) fn queue_to_simh(
        &mut self,
        port: BackendSerialPort,
        byte: u8,
    ) -> Result<(), SimhSerialBridgeError> {
        self.port_mut(port).queue_to_simh(byte)
    }

    pub(crate) fn to_simh_len(&self, port: BackendSerialPort) -> usize {
        self.port(port).to_simh.len()
    }

    pub(crate) fn from_simh_len(&self, port: BackendSerialPort) -> usize {
        self.port(port).from_simh.len()
    }

    pub(crate) fn from_simh_front(&self, port: BackendSerialPort) -> Option<u8> {
        self.port(port).from_simh.front().copied()
    }

    pub(crate) fn pop_from_simh(&mut self, port: BackendSerialPort) -> Option<u8> {
        self.port_mut(port).from_simh.pop_front()
    }

    pub(crate) fn clear_queues(&mut self) {
        self.port0.clear_queues();
        self.port1.clear_queues();
    }

    pub(crate) fn disconnect(&mut self) {
        self.port0.disconnect();
        self.port1.disconnect();
    }

    fn port(&self, port: BackendSerialPort) -> &SimhRawTcpPort {
        match port {
            BackendSerialPort::Port0 => &self.port0,
            BackendSerialPort::Port1 => &self.port1,
        }
    }

    fn port_mut(&mut self, port: BackendSerialPort) -> &mut SimhRawTcpPort {
        match port {
            BackendSerialPort::Port0 => &mut self.port0,
            BackendSerialPort::Port1 => &mut self.port1,
        }
    }
}

/// Temporary config overlay used only for the lifetime of a SIMH backend.
/// The caller's original simulator config is copied byte-for-byte and remains
/// untouched. Open-SIMH then connects M2SIO0/1 outward to RusTair's two
/// loopback-only listeners using TMXR raw TCP (`;notelnet`).
pub(crate) struct SimhM2SioRuntimeConfig {
    path: PathBuf,
}

impl SimhM2SioRuntimeConfig {
    pub(crate) fn create(
        base_config: &Path,
        port0: u16,
        port1: u16,
    ) -> Result<Self, SimhSerialBridgeError> {
        let mut contents = fs::read(base_config)?;
        if !contents.ends_with(b"\n") {
            contents.push(b'\n');
        }
        let extra = format!(
            "set m2sio0 enabled\n\
set m2sio1 enabled\n\
set m2sio0 noconsole\n\
set m2sio1 noconsole\n\
set m2sio0 dcd\n\
set m2sio0 cts\n\
set m2sio1 dcd\n\
set m2sio1 cts\n\
attach m2sio0 Connect=127.0.0.1:{port0};notelnet\n\
attach m2sio1 Connect=127.0.0.1:{port1};notelnet\n"
        );
        contents.extend_from_slice(extra.as_bytes());

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-m2sio-{}-{nonce}.ini",
            std::process::id()
        ));
        fs::write(&path, contents)?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path { &self.path }
}

impl Drop for SimhM2SioRuntimeConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_bridge_moves_raw_bytes_both_directions() {
        let mut bridge = SimhM2SioBridge::bind_loopback().expect("bind bridge");
        let (port0, port1) = bridge.listen_ports();
        let mut peer0 = TcpStream::connect((Ipv4Addr::LOCALHOST, port0)).expect("connect port0");
        let _peer1 = TcpStream::connect((Ipv4Addr::LOCALHOST, port1)).expect("connect port1");
        bridge
            .wait_for_connections(Duration::from_secs(1))
            .expect("accept bridge peers");

        bridge
            .queue_to_simh(BackendSerialPort::Port0, 0xa5)
            .expect("queue to SIMH");
        bridge.poll().expect("flush to SIMH");
        peer0
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout");
        let mut byte = [0u8; 1];
        peer0.read_exact(&mut byte).expect("read bridge byte");
        assert_eq!(byte[0], 0xa5);

        peer0.write_all(&[0x5a]).expect("write bridge byte");
        let deadline = Instant::now() + Duration::from_secs(1);
        while bridge.from_simh_len(BackendSerialPort::Port0) == 0 && Instant::now() < deadline {
            bridge.poll().expect("poll bridge receive");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(bridge.pop_from_simh(BackendSerialPort::Port0), Some(0x5a));
    }
}
