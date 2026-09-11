//! Both ends of one SSDP exchange on this machine (ADR-0051): a control
//! point at an ephemeral local port, and a device that announces itself to
//! it and holds the Stream. SSDP says what a device is rather than what it
//! sends, and a Stream arrives as what it says it in: the control point
//! searches for the Stream chunk by chunk, an `M-SEARCH` whose `ST` names
//! the chunk, and the device answers each with one response carrying
//! [`CHUNK`] bytes in hex under one vendor header; a search past the end is
//! answered with no such header, and that closes it. Every chunk is asked
//! for: announcing them unasked lost the tail of a mebibyte on loopback
//! (2026-09-09), because nobody acknowledges a notification and the
//! socket buffer was the only flow control there was.

use std::fmt::Write;
use std::net::UdpSocket;

use transport::Arrived;
use transport::error::{Result, classify, protocol_error};
use transport::loopback::{FarEnd, LOOPBACK_TIMEOUT, Loopback};
use transport::socket;

use crate::message::{self, Message};
use crate::{MAX_DATAGRAM, SsdpTransport};

/// The vendor header a response carries the Stream in.
const HEADER: &str = "X-STREAM";
/// What one response carries: twice this in hex and the headers stay well
/// inside the datagram the transport takes.
const CHUNK: usize = 3000;
/// The search type chunk `N` answers to, `{STREAM}N`.
const STREAM: &str = "urn:xmip:stream:";
/// What the device announces itself as.
const DEVICE: &str = "urn:xmip:device:Probe:1";
const USN: &str = "uuid:xmip-probe::urn:xmip:device:Probe:1";
const LOCATION: &str = "http://127.0.0.1/probe.xml";

impl SsdpTransport {
    /// Both ends on this machine: a control point at an ephemeral local
    /// port, the loopback timeout on both.
    #[must_use]
    pub fn loopback() -> Self {
        Self::new("127.0.0.1:0").timing_out_after(LOOPBACK_TIMEOUT)
    }
}

/// A bound control point waiting for a device to announce itself.
struct ControlPoint {
    transport: SsdpTransport,
    socket: UdpSocket,
    address: String,
}

impl FarEnd for ControlPoint {
    fn address(&self) -> &str {
        &self.address
    }

    /// Take the announcement, then search the device that made it for the
    /// Stream, a chunk per search.
    fn take_one(self: Box<Self>) -> Result<Arrived> {
        let alive = self.transport.receive_datagram(&self.socket)?;
        let device = peer_of(&alive.origin_uri)?;
        let mut bytes = Vec::new();
        for n in 0..usize::MAX {
            let search = Message::search(&format!("{STREAM}{n}"), 1);
            self.socket
                .send_to(&message::format(&search), &device)
                .map_err(|e| classify("searching", &e))?;
            let answer = self.transport.receive_datagram(&self.socket)?;
            let response = message::parse(&answer.bytes)?;
            let Some(chunk) = response.header(HEADER) else {
                return Ok(Arrived::new(alive.origin_uri, bytes));
            };
            bytes.extend(unhex(chunk)?);
        }
        Err(protocol_error("a Stream that never ends"))
    }
}

impl Loopback for SsdpTransport {
    fn far_end(&self) -> Result<Box<dyn FarEnd>> {
        let (socket, address) = self.bind_udp()?;
        Ok(Box::new(ControlPoint {
            transport: self.clone(),
            socket,
            address,
        }))
    }

    /// A device announces itself alive to the control point at `address`,
    /// then answers its searches until one asks past the end.
    fn send_to(&self, address: &str, payload: &[u8]) -> Result<()> {
        let (device, _) = socket::bind_udp("127.0.0.1:0", self.timeout)?;
        let alive = Message::alive(DEVICE, USN, LOCATION, &self.server);
        device
            .send_to(&message::format(&alive), address)
            .map_err(|e| classify("announcing", &e))?;
        let chunks = payload.chunks(CHUNK).count();
        let mut buffer = vec![0u8; MAX_DATAGRAM];
        loop {
            let (read, peer) = device
                .recv_from(&mut buffer)
                .map_err(|e| classify("awaiting a search", &e))?;
            let search = message::parse(&buffer[..read])?;
            let n = chunk_asked(search.notification_type())?;
            device
                .send_to(&self.response(payload, n), peer)
                .map_err(|e| classify("answering a search", &e))?;
            if n >= chunks {
                return Ok(());
            }
        }
    }

    fn unblock(&self, _address: &str) {
        // The receive has its own timeout; there is no listener to poke.
    }
}

impl SsdpTransport {
    /// The response to a search for chunk `n` of `payload`.
    fn response(&self, payload: &[u8], n: usize) -> Vec<u8> {
        let response = Message::response(&format!("{STREAM}{n}"), USN, LOCATION, &self.server);
        let response = match payload.chunks(CHUNK).nth(n) {
            Some(chunk) => response.with(HEADER, &hex(chunk)),
            None => response,
        };
        message::format(&response)
    }
}

/// The peer an origin `ssdp://peer?…` names.
fn peer_of(origin: &str) -> Result<String> {
    socket::target("ssdp", origin)
        .map(|(authority, _)| authority.split_once('?').map_or(authority, |(a, _)| a))
        .filter(|peer| !peer.is_empty())
        .map(str::to_string)
        .ok_or_else(|| protocol_error(format!("an origin naming no peer: {origin}")))
}

/// The chunk a search type asks for, where it asks for one.
fn chunk_asked(st: &str) -> Result<usize> {
    st.strip_prefix(STREAM)
        .and_then(|rest| rest.parse().ok())
        .ok_or_else(|| protocol_error(format!("a search for something else: {st:?}")))
}

/// `bytes` as lower-case hex pairs, the form a header value takes.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The bytes `digits` spell, refused where they do not.
fn unhex(digits: &str) -> Result<Vec<u8>> {
    if !digits.len().is_multiple_of(2) {
        return Err(protocol_error(format!(
            "an odd number of hex digits: {digits:?}"
        )));
    }
    (0..digits.len())
        .step_by(2)
        .map(|at| {
            digits
                .get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| protocol_error(format!("not hex: {digits:?}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_reads_back_and_a_search_names_its_chunk() {
        assert_eq!(hex(&[0, 0x7f, 0xff]), "007fff");
        assert_eq!(unhex("007fff").expect("hex"), [0, 0x7f, 0xff]);
        assert!(unhex("").expect("nothing").is_empty());
        assert!(unhex("abc").is_err(), "odd");
        assert!(unhex("zz").is_err(), "not hex");
        assert_eq!(chunk_asked("urn:xmip:stream:12").expect("asked"), 12);
        assert!(chunk_asked("ssdp:all").is_err());
        assert_eq!(
            peer_of("ssdp://127.0.0.1:1900?nt=x&usn=y&nts=ssdp:alive").expect("peer"),
            "127.0.0.1:1900"
        );
        assert!(peer_of("ssdp://?nt=x").is_err());
        assert!(peer_of("http://127.0.0.1:1900").is_err());
    }
}
