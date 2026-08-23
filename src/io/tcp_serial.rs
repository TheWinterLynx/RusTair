use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};

use crate::config::{ExternalSerialConfig, TcpListenScope};

const SOCKET_CHUNK: usize = 4096;
const RX_QUEUE_LIMIT: usize = 64 * 1024;
const CLIENT_TX_QUEUE_LIMIT: usize = 64 * 1024;
const NETWORK_TRACE_LIMIT: usize = 4096;

struct TcpClient {
    stream: TcpStream,
    peer: SocketAddr,
    tx_queue: VecDeque<u8>,
}

#[derive(Clone, Copy, Debug)]
struct TcpRxByte {
    byte: u8,
    peer: SocketAddr,
}

#[derive(Clone, Copy, Debug)]
struct NetworkTraceEvent {
    sequence: u64,
    inbound: bool,
    byte: u8,
    peer: Option<SocketAddr>,
}

/// Non-blocking raw TCP transport for one logical external serial endpoint.
///
/// The transport deliberately knows nothing about MITS I/O ports. It only
/// accepts raw host bytes and exposes queues/counters to the app layer, which
/// applies character semantics, serial pacing and routing to an emulated UART.
/// RX entries retain their originating peer for diagnostics and trace output.
pub(crate) struct TcpSerialServer {
    listener: Option<TcpListener>,
    active_bind: Option<(TcpListenScope, u16)>,
    clients: Vec<TcpClient>,
    rx_queue: VecDeque<TcpRxByte>,
    rx_bytes: u64,
    tx_bytes: u64,
    rejected_clients: u64,
    dropped_tx_bytes: u64,
    last_error: Option<String>,
    network_trace_enabled: bool,
    network_trace: VecDeque<NetworkTraceEvent>,
    next_trace_sequence: u64,
}

impl Default for TcpSerialServer {
    fn default() -> Self {
        Self {
            listener: None,
            active_bind: None,
            clients: Vec::new(),
            rx_queue: VecDeque::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rejected_clients: 0,
            dropped_tx_bytes: 0,
            last_error: None,
            network_trace_enabled: false,
            network_trace: VecDeque::new(),
            next_trace_sequence: 1,
        }
    }
}

impl TcpSerialServer {
    fn record_network_byte(&mut self, inbound: bool, byte: u8, peer: Option<SocketAddr>) {
        if !self.network_trace_enabled {
            return;
        }

        self.network_trace.push_back(NetworkTraceEvent {
            sequence: self.next_trace_sequence,
            inbound,
            byte,
            peer,
        });
        self.next_trace_sequence = self.next_trace_sequence.saturating_add(1);
        while self.network_trace.len() > NETWORK_TRACE_LIMIT {
            self.network_trace.pop_front();
        }
    }

    pub(crate) fn poll(&mut self, config: ExternalSerialConfig) {
        self.sync_config(config);
        if !config.enabled || self.listener.is_none() {
            return;
        }

        self.enforce_client_policy(config.allow_multiple_clients);
        self.accept_clients(config.allow_multiple_clients);
        self.read_clients();
        self.flush_clients();
    }

    fn sync_config(&mut self, config: ExternalSerialConfig) {
        if !config.enabled {
            if self.listener.is_some() || self.active_bind.is_some() || !self.clients.is_empty() {
                self.stop();
            }
            return;
        }

        let desired = (config.listen_scope, config.tcp_port);
        if self.active_bind != Some(desired) {
            self.bind(config.listen_scope, config.tcp_port);
        }
    }

    fn bind(&mut self, scope: TcpListenScope, port: u16) {
        self.close_clients();
        self.rx_queue.clear();
        self.listener = None;
        self.active_bind = Some((scope, port));
        self.last_error = None;

        let address = SocketAddr::from((scope.bind_ipv4(), port));
        match TcpListener::bind(address) {
            Ok(listener) => {
                if let Err(error) = listener.set_nonblocking(true) {
                    self.last_error = Some(format!("Could not make TCP listener non-blocking: {error}"));
                    return;
                }
                self.listener = Some(listener);
            }
            Err(error) => {
                self.last_error = Some(format!("Could not listen on {address}: {error}"));
            }
        }
    }

    fn accept_clients(&mut self, allow_multiple_clients: bool) {
        loop {
            let accepted = match self.listener.as_ref() {
                Some(listener) => listener.accept(),
                None => return,
            };

            match accepted {
                Ok((stream, peer)) => {
                    if !allow_multiple_clients && !self.clients.is_empty() {
                        let _ = stream.shutdown(Shutdown::Both);
                        self.rejected_clients = self.rejected_clients.saturating_add(1);
                        continue;
                    }

                    if let Err(error) = stream.set_nonblocking(true) {
                        self.last_error = Some(format!(
                            "Rejected {peer}: could not make client socket non-blocking: {error}"
                        ));
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    let _ = stream.set_nodelay(true);
                    self.clients.push(TcpClient {
                        stream,
                        peer,
                        tx_queue: VecDeque::new(),
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => {
                    self.last_error = Some(format!("TCP accept failed: {error}"));
                    break;
                }
            }
        }
    }

    fn enforce_client_policy(&mut self, allow_multiple_clients: bool) {
        if allow_multiple_clients || self.clients.len() <= 1 {
            return;
        }

        for client in self.clients.drain(1..) {
            let _ = client.stream.shutdown(Shutdown::Both);
        }
    }

    fn read_clients(&mut self) {
        let mut index = 0;
        while index < self.clients.len() {
            let room = RX_QUEUE_LIMIT.saturating_sub(self.rx_queue.len());
            if room == 0 {
                break;
            }

            let peer = self.clients[index].peer;
            let mut buffer = [0_u8; SOCKET_CHUNK];
            let max_read = room.min(buffer.len());
            let result = self.clients[index].stream.read(&mut buffer[..max_read]);

            match result {
                Ok(0) => {
                    self.clients.remove(index);
                }
                Ok(count) => {
                    for &byte in &buffer[..count] {
                        self.record_network_byte(true, byte, Some(peer));
                        self.rx_queue.push_back(TcpRxByte { byte, peer });
                    }
                    self.rx_bytes = self.rx_bytes.saturating_add(count as u64);
                    index += 1;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    index += 1;
                }
                Err(_) => {
                    self.clients.remove(index);
                }
            }
        }
    }

    pub(crate) fn flush_clients(&mut self) {
        let mut index = 0;
        while index < self.clients.len() {
            let mut disconnected = false;

            loop {
                if self.clients[index].tx_queue.is_empty() {
                    break;
                }

                let chunk: Vec<u8> = self.clients[index]
                    .tx_queue
                    .iter()
                    .take(SOCKET_CHUNK)
                    .copied()
                    .collect();

                match self.clients[index].stream.write(&chunk) {
                    Ok(0) => {
                        disconnected = true;
                        break;
                    }
                    Ok(count) => {
                        for _ in 0..count {
                            self.clients[index].tx_queue.pop_front();
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                self.clients.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// Queue one guest-transmitted serial byte for every connected network
    /// client. `tx_bytes` counts logical Altair bytes, not N copies for N
    /// clients, so the counter remains meaningful when fan-out is enabled.
    pub(crate) fn broadcast_byte(&mut self, byte: u8) {
        self.tx_bytes = self.tx_bytes.saturating_add(1);
        self.record_network_byte(false, byte, None);
        for client in &mut self.clients {
            if client.tx_queue.len() < CLIENT_TX_QUEUE_LIMIT {
                client.tx_queue.push_back(byte);
            } else {
                self.dropped_tx_bytes = self.dropped_tx_bytes.saturating_add(1);
            }
        }
    }

    pub(crate) fn pop_rx(&mut self) -> Option<(u8, SocketAddr)> {
        self.rx_queue.pop_front().map(|entry| (entry.byte, entry.peer))
    }

    pub(crate) fn clear_rx(&mut self) {
        self.rx_queue.clear();
    }

    pub(crate) fn rx_pending(&self) -> usize {
        self.rx_queue.len()
    }

    pub(crate) fn rx_bytes(&self) -> u64 {
        self.rx_bytes
    }

    pub(crate) fn tx_bytes(&self) -> u64 {
        self.tx_bytes
    }

    pub(crate) fn rejected_clients(&self) -> u64 {
        self.rejected_clients
    }

    pub(crate) fn dropped_tx_bytes(&self) -> u64 {
        self.dropped_tx_bytes
    }

    pub(crate) fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub(crate) fn peer_addresses(&self) -> Vec<SocketAddr> {
        self.clients.iter().map(|client| client.peer).collect()
    }

    pub(crate) fn listening(&self) -> bool {
        self.listener.is_some()
    }

    pub(crate) fn active_bind(&self) -> Option<SocketAddr> {
        self.active_bind
            .map(|(scope, port)| SocketAddr::from((scope.bind_ipv4(), port)))
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(crate) fn network_trace_enabled(&self) -> bool {
        self.network_trace_enabled
    }

    pub(crate) fn set_network_trace_enabled(&mut self, enabled: bool) {
        self.network_trace_enabled = enabled;
    }

    pub(crate) fn network_trace_snapshot(&self) -> Vec<(u64, bool, u8, Option<SocketAddr>)> {
        self.network_trace
            .iter()
            .map(|event| (event.sequence, event.inbound, event.byte, event.peer))
            .collect()
    }

    pub(crate) fn clear_network_trace(&mut self) {
        self.network_trace.clear();
    }

    pub(crate) fn disconnect_all(&mut self) {
        self.close_clients();
        self.rx_queue.clear();
    }

    /// Close everything and make the next `poll` perform a fresh bind using the
    /// current configuration. Useful after a transient bind error.
    pub(crate) fn restart_on_next_poll(&mut self) {
        self.close_clients();
        self.rx_queue.clear();
        self.listener = None;
        self.active_bind = None;
        self.last_error = None;
    }

    fn stop(&mut self) {
        self.close_clients();
        self.rx_queue.clear();
        self.listener = None;
        self.active_bind = None;
        self.last_error = None;
    }

    fn close_clients(&mut self) {
        for client in self.clients.drain(..) {
            let _ = client.stream.shutdown(Shutdown::Both);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_trace_is_opt_in() {
        let mut server = TcpSerialServer::default();
        server.broadcast_byte(b'A');
        assert!(server.network_trace_snapshot().is_empty());

        server.set_network_trace_enabled(true);
        server.broadcast_byte(b'B');
        let trace = server.network_trace_snapshot();
        assert_eq!(trace.len(), 1);
        assert!(!trace[0].1);
        assert_eq!(trace[0].2, b'B');
        assert!(trace[0].3.is_none());
    }
}
