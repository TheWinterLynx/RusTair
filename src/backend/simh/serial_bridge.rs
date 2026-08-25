use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::BackendSerialPort;

const SOCKET_CHUNK: usize = 4096;
const QUEUE_LIMIT: usize = 64 * 1024;
/// TMXR opens a disposable connection while validating `Connect=host:port`.
/// On Windows the peer FIN may not be observable immediately after `accept()`.
/// Keep a newly accepted socket in a probationary state long enough to observe
/// that close before advertising the line as connected or sending guest input.
const CONNECTION_SETTLE: Duration = Duration::from_millis(75);

#[derive(Debug)]
pub(crate) enum SimhSerialBridgeError {
    Io(std::io::Error),
    QueueFull { port: BackendSerialPort },
}

impl fmt::Display for SimhSerialBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::QueueFull { port } => write!(f, "SIMH serial queue is full for {port:?}"),
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
    accepted_at: Option<Instant>,
    ready: bool,
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
            accepted_at: None,
            ready: false,
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

    fn connected(&self) -> bool { self.ready && self.stream.is_some() }

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
                self.accepted_at = Some(Instant::now());
                self.ready = false;
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

    /// Poll one raw TCP line without treating a peer disconnect as fatal.
    ///
    /// Open-SIMH TMXR deliberately opens and closes a short-lived validation
    /// connection while parsing `Connect=host:port`, then establishes the real
    /// M2SIO connection later from `tmxr_poll_conn()` while the simulator is
    /// executing. A newly accepted socket is therefore probationary: we probe
    /// it for EOF for `CONNECTION_SETTLE` before advertising it as connected or
    /// flushing RusTair-to-SIMH bytes into it.
    fn poll(&mut self) -> Result<(), SimhSerialBridgeError> {
        self.accept_pending()?;
        if self.stream.is_none() {
            return Ok(());
        }

        // Probe/read first so the disposable ATTACH validation connection can
        // disappear without consuming any queued guest input.
        self.read_from_simh()?;
        if self.stream.is_none() {
            return Ok(());
        }

        if !self.ready {
            let settled = self
                .accepted_at
                .map(|accepted_at| accepted_at.elapsed() >= CONNECTION_SETTLE)
                .unwrap_or(false);
            if !settled {
                return Ok(());
            }
            self.ready = true;
        }

        self.flush_to_simh()?;
        if self.stream.is_none() {
            return Ok(());
        }

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
                Ok(0) => {
                    self.drop_stream();
                    return Ok(());
                }
                Ok(count) => {
                    for _ in 0..count {
                        self.to_simh.pop_front();
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.drop_stream();
                    return Ok(());
                }
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
                Ok(0) => {
                    self.drop_stream();
                    return Ok(());
                }
                Ok(count) => self.from_simh.extend(buffer[..count].iter().copied()),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(_) => {
                    self.drop_stream();
                    return Ok(());
                }
            }
        }
    }

    fn drop_stream(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.accepted_at = None;
        self.ready = false;
    }

    fn disconnect(&mut self) {
        self.drop_stream();
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
/// loopback-only listeners using TMXR raw TCP (`;notelnet`). DTR is configured
/// to follow the guest-controlled RTS signal; TMXR modem-control passthrough
/// otherwise suppresses outgoing connections while DTR is inactive.
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
set m2sio0 dtr\n\
set m2sio1 dtr\n\
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
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    fn poll_until(
        bridge: &mut SimhM2SioBridge,
        predicate: impl Fn(&SimhM2SioBridge) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !predicate(bridge) && Instant::now() < deadline {
            bridge.poll().expect("poll bridge");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(predicate(bridge), "bridge condition did not become true before timeout");
    }

    #[test]
    fn loopback_bridge_moves_raw_bytes_both_directions() {
        let mut bridge = SimhM2SioBridge::bind_loopback().expect("bind bridge");
        let (port0, port1) = bridge.listen_ports();
        let mut peer0 = TcpStream::connect((Ipv4Addr::LOCALHOST, port0)).expect("connect port0");
        let _peer1 = TcpStream::connect((Ipv4Addr::LOCALHOST, port1)).expect("connect port1");
        poll_until(&mut bridge, |bridge| {
            bridge.connected(BackendSerialPort::Port0)
                && bridge.connected(BackendSerialPort::Port1)
        });

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
        poll_until(&mut bridge, |bridge| {
            bridge.from_simh_len(BackendSerialPort::Port0) != 0
        });
        assert_eq!(bridge.pop_from_simh(BackendSerialPort::Port0), Some(0x5a));
    }

    #[test]
    fn queued_byte_survives_slow_tmxr_validation_socket_and_reconnect() {
        let mut bridge = SimhM2SioBridge::bind_loopback().expect("bind bridge");
        let (port0, port1) = bridge.listen_ports();

        let mut validation0 = TcpStream::connect((Ipv4Addr::LOCALHOST, port0)).expect("validation port0");
        let validation1 = TcpStream::connect((Ipv4Addr::LOCALHOST, port1)).expect("validation port1");
        validation0
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("validation read timeout");

        bridge
            .queue_to_simh(BackendSerialPort::Port0, 0xa5)
            .expect("queue before validation socket is discarded");

        // Keep the disposable peers alive briefly. RusTair must not advertise
        // them as ready or flush queued bytes merely because accept() succeeded.
        let probation_deadline = Instant::now() + Duration::from_millis(25);
        while Instant::now() < probation_deadline {
            bridge.poll().expect("poll validation socket during probation");
            assert!(!bridge.connected(BackendSerialPort::Port0));
            assert_eq!(bridge.to_simh_len(BackendSerialPort::Port0), 1);
            thread::sleep(Duration::from_millis(1));
        }
        let mut unexpected = [0u8; 1];
        assert!(
            validation0.read_exact(&mut unexpected).is_err(),
            "queued byte leaked into the disposable TMXR validation socket"
        );

        drop(validation0);
        drop(validation1);

        let mut persistent0 = TcpStream::connect((Ipv4Addr::LOCALHOST, port0)).expect("persistent port0");
        let _persistent1 = TcpStream::connect((Ipv4Addr::LOCALHOST, port1)).expect("persistent port1");
        poll_until(&mut bridge, |bridge| {
            bridge.connected(BackendSerialPort::Port0)
                && bridge.connected(BackendSerialPort::Port1)
                && bridge.to_simh_len(BackendSerialPort::Port0) == 0
        });

        persistent0
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout");
        let mut byte = [0u8; 1];
        persistent0.read_exact(&mut byte).expect("read queued byte after reconnect");
        assert_eq!(byte[0], 0xa5);
    }

    #[test]
    fn runtime_config_enables_dtr_following_rts_for_both_ports() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let base = env::temp_dir().join(format!(
            "rustair-simh-m2sio-base-{}-{nonce}.ini",
            std::process::id()
        ));
        fs::write(&base, b"set cpu 8080\n").expect("write base config");

        let runtime = SimhM2SioRuntimeConfig::create(&base, 12345, 12346)
            .expect("create runtime M2SIO config");
        let contents = fs::read_to_string(runtime.path()).expect("read runtime config");

        assert!(contents.contains("set m2sio0 dtr\n"));
        assert!(contents.contains("set m2sio1 dtr\n"));
        assert!(contents.contains("attach m2sio0 Connect=127.0.0.1:12345;notelnet\n"));
        assert!(contents.contains("attach m2sio1 Connect=127.0.0.1:12346;notelnet\n"));

        drop(runtime);
        fs::remove_file(base).expect("remove base config");
    }
}
