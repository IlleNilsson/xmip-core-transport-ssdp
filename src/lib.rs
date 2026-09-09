#![forbid(unsafe_code)]

//! Streams that arrive as SSDP messages. One notification or search
//! response is one Stream: the whole HTTPU text, with what its headers
//! said in the origin.
//!
//! SSDP is how `UPnP` devices find each other: a device multicasts `NOTIFY`
//! `ssdp:alive` when it arrives and `ssdp:byebye` when it leaves, a control
//! point multicasts `M-SEARCH` and each device answers it unicast with
//! `HTTP/1.1 200 OK`. A Receive Location joins the group and takes what the
//! network announces — printers, media renderers, gateways, cameras — or
//! sits at a unicast address and takes the answers to its own searches; a
//! Send Location announces a device or searches for one.
//!
//! The origin URI carries what the headers knew:
//! `ssdp://peer?nt=upnp:rootdevice&usn=uuid:…::upnp:rootdevice&nts=ssdp:alive`
//! — `nt` is `ST` for a search response, and `nts` is empty for one.
//!
//! A send goes to `ssdp://host:port` or a bare `host:port`; the group is
//! `ssdp://239.255.255.250:1900`. Bytes that already open with `NOTIFY`,
//! `M-SEARCH` or `HTTP/` go as they are; any other bytes go as a `NOTIFY`
//! `ssdp:alive` whose `LOCATION` they are, under the `NT` and `USN` the
//! transport was built announcing.
//!
//! Multicast is joined when the bind address is in 224/4 — the socket
//! binds the port on every interface and joins the group — and is never
//! used under test; a test binds `127.0.0.1:0` and sends from a second
//! socket.

pub mod message;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

pub use message::{ALIVE, ALL, BYEBYE, GROUP, Kind, Message};
use transport::error::{Result, classify, protocol_error};
use transport::socket;
use transport::{Arrived, Directions, Transport};

/// The largest datagram taken; an SSDP message is a few hundred bytes.
pub const MAX_DATAGRAM: usize = 8192;

/// What this node says it is, when asked or when announcing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub st: String,
    pub usn: String,
    pub location: String,
}

pub struct SsdpTransport {
    bind: String,
    announcing: Option<Announcement>,
    server: String,
    timeout: Option<Duration>,
}

impl SsdpTransport {
    /// Listen at `bind`: the group `239.255.255.250:1900` for what the
    /// network announces, or a unicast address for answers to a search.
    #[must_use]
    pub fn new(bind: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
            announcing: None,
            server: "xmip/0.1 UPnP/1.1".to_string(),
            timeout: None,
        }
    }

    /// Answer an `M-SEARCH` for `st` (or `ssdp:all`) with `usn` at
    /// `location`, and announce the same when sent bytes that are only a
    /// location.
    #[must_use]
    pub fn announcing(mut self, st: &str, usn: &str, location: &str) -> Self {
        self.announcing = Some(Announcement {
            st: st.to_string(),
            usn: usn.to_string(),
            location: location.to_string(),
        });
        self
    }

    /// Give up waiting for a message after `timeout`.
    #[must_use]
    pub const fn timing_out_after(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Bind the UDP socket, joining the group when the bind address is a
    /// multicast one, and report the address actually assigned.
    ///
    /// # Errors
    /// Where the address is taken, malformed, or not permitted, or the
    /// group could not be joined.
    pub fn bind_udp(&self) -> Result<(UdpSocket, String)> {
        let Some((group, port)) = multicast_group(&self.bind) else {
            return socket::bind_udp(&self.bind, self.timeout);
        };
        let (socket, local) = socket::bind_udp(&port, self.timeout)?;
        socket
            .join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
            .map_err(|e| classify("joining the group", &e))?;
        Ok((socket, local))
    }

    /// Take one notification or search response from an already-bound
    /// socket. A search arriving is answered where this node announces
    /// something it asks for, and skipped.
    ///
    /// # Errors
    /// Where nothing arrived in time, or what arrived is not HTTPU.
    pub fn receive_datagram(&self, socket: &UdpSocket) -> Result<Arrived> {
        let mut buffer = vec![0u8; MAX_DATAGRAM];
        loop {
            let (read, peer) = socket
                .recv_from(&mut buffer)
                .map_err(|e| classify("receiving a datagram", &e))?;
            let message = message::parse(&buffer[..read])?;
            if message.kind != Kind::Search {
                return Ok(arrived(peer, &message, &buffer[..read]));
            }
            if let Some(answer) = self.answer(&message) {
                socket
                    .send_to(&message::format(&answer), peer)
                    .map_err(|e| classify("answering a search", &e))?;
            }
        }
    }

    /// The response `search` earns, where it asks for what is announced.
    #[must_use]
    pub fn answer(&self, search: &Message) -> Option<Message> {
        let announcement = self.announcing.as_ref()?;
        let st = search.notification_type();
        (st == ALL || st == announcement.st).then(|| {
            Message::response(
                &announcement.st,
                &announcement.usn,
                &announcement.location,
                &self.server,
            )
        })
    }

    /// `bytes` as a message: as they are when they already are one, else
    /// an `ssdp:alive` at that location.
    #[must_use]
    pub fn compose(&self, bytes: &[u8]) -> Vec<u8> {
        if message::is_message(bytes) {
            return bytes.to_vec();
        }
        let location = String::from_utf8_lossy(bytes);
        let (nt, usn) = self.announcing.as_ref().map_or(
            (
                "upnp:rootdevice".to_string(),
                "uuid:xmip::upnp:rootdevice".to_string(),
            ),
            |a| (a.st.clone(), a.usn.clone()),
        );
        message::format(&Message::alive(&nt, &usn, location.trim(), &self.server))
    }
}

/// The group and the `0.0.0.0:port` to bind for it, where `bind` is a
/// multicast address.
#[must_use]
pub fn multicast_group(bind: &str) -> Option<(Ipv4Addr, String)> {
    let address: SocketAddrV4 = bind.parse().ok()?;
    address
        .ip()
        .is_multicast()
        .then(|| (*address.ip(), format!("0.0.0.0:{}", address.port())))
}

fn arrived(peer: SocketAddr, message: &Message, raw: &[u8]) -> Arrived {
    Arrived::new(
        format!(
            "ssdp://{peer}?nt={}&usn={}&nts={}",
            message.notification_type(),
            message.header("USN").unwrap_or(""),
            message.header("NTS").unwrap_or("")
        ),
        raw,
    )
}

impl Transport for SsdpTransport {
    fn name(&self) -> &'static str {
        "ssdp"
    }

    fn directions(&self) -> Directions {
        Directions::BOTH
    }

    fn receive(&self) -> Result<Vec<Arrived>> {
        let (socket, _) = self.bind_udp()?;
        Ok(vec![self.receive_datagram(&socket)?])
    }

    fn send(&self, target: &str, bytes: &[u8]) -> Result<()> {
        let address = match socket::target("ssdp", target) {
            Some((address, _)) => address,
            None if target.contains("://") => {
                return Err(protocol_error(format!("not an ssdp target: {target}")));
            }
            None => target,
        };
        let datagram = self.compose(bytes);
        let sender =
            UdpSocket::bind("0.0.0.0:0").map_err(|e| classify("binding the sending socket", &e))?;
        sender
            .send_to(&datagram, address)
            .map_err(|e| classify("sending the datagram", &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> SsdpTransport {
        SsdpTransport::new("127.0.0.1:0").timing_out_after(Duration::from_secs(2))
    }

    fn printer() -> SsdpTransport {
        node().announcing(
            "urn:schemas-upnp-org:device:Printer:1",
            "uuid:1234::urn:schemas-upnp-org:device:Printer:1",
            "http://10.0.0.5:8080/desc.xml",
        )
    }

    #[test]
    fn a_location_is_announced_alive_and_a_message_goes_as_it_is() {
        let far_end = node();
        let (socket, address) = far_end.bind_udp().expect("binding");
        printer()
            .send(
                &format!("ssdp://{address}"),
                b"http://10.0.0.5:8080/desc.xml",
            )
            .expect("announcing");
        let arrived = far_end.receive_datagram(&socket).expect("receiving");
        assert!(
            arrived.origin_uri.ends_with(
                "?nt=urn:schemas-upnp-org:device:Printer:1\
                 &usn=uuid:1234::urn:schemas-upnp-org:device:Printer:1&nts=ssdp:alive"
            ),
            "{}",
            arrived.origin_uri
        );
        assert!(arrived.bytes.starts_with(b"NOTIFY * HTTP/1.1\r\n"));
        let text = String::from_utf8(arrived.bytes).expect("text");
        assert!(text.contains("LOCATION: http://10.0.0.5:8080/desc.xml\r\n"));
        assert!(text.contains("SERVER: xmip/0.1 UPnP/1.1\r\n"));
        let byebye = message::format(&Message::byebye(
            "upnp:rootdevice",
            "uuid:9::upnp:rootdevice",
        ));
        node().send(&address, &byebye).expect("as it is");
        let gone = far_end.receive_datagram(&socket).expect("receiving");
        assert_eq!(gone.bytes, byebye);
        assert!(
            gone.origin_uri
                .ends_with("?nt=upnp:rootdevice&usn=uuid:9::upnp:rootdevice&nts=ssdp:byebye")
        );
        node()
            .send(&address, b"http://x/")
            .expect("no announcement");
        let plain = far_end.receive_datagram(&socket).expect("receiving");
        assert!(
            plain
                .origin_uri
                .contains("nt=upnp:rootdevice&usn=uuid:xmip::upnp:rootdevice")
        );
    }

    #[test]
    fn a_search_is_answered_where_something_is_announced() {
        let far_end = printer();
        let (socket, address) = far_end.bind_udp().expect("binding");
        let searcher = UdpSocket::bind("127.0.0.1:0").expect("searcher");
        searcher
            .set_read_timeout(Some(Duration::from_millis(300)))
            .expect("timeout");
        let searching = std::thread::spawn(move || {
            let mut buffer = [0u8; MAX_DATAGRAM];
            searcher
                .send_to(
                    &message::format(&Message::search("urn:other:1", 1)),
                    &address,
                )
                .expect("other");
            assert!(searcher.recv(&mut buffer).is_err(), "not what is announced");
            searcher
                .send_to(&message::format(&Message::search(ALL, 1)), &address)
                .expect("all");
            let read = searcher.recv(&mut buffer).expect("answered");
            let answer = message::parse(&buffer[..read]).expect("parse");
            assert_eq!(answer.kind, Kind::Response);
            assert_eq!(
                answer.header("ST"),
                Some("urn:schemas-upnp-org:device:Printer:1")
            );
            assert_eq!(
                answer.header("LOCATION"),
                Some("http://10.0.0.5:8080/desc.xml")
            );
            searcher
                .send_to(&message::format(&answer), &address)
                .expect("a response arriving");
        });
        let arrived = far_end.receive_datagram(&socket).expect("the response");
        assert!(arrived.origin_uri.ends_with(
            "?nt=urn:schemas-upnp-org:device:Printer:1\
             &usn=uuid:1234::urn:schemas-upnp-org:device:Printer:1&nts="
        ));
        searching.join().expect("thread");
        assert!(
            node().answer(&Message::search(ALL, 1)).is_none(),
            "nothing announced"
        );
    }

    #[test]
    fn what_is_not_httpu_is_refused_and_the_group_is_known() {
        let far_end = node();
        let (socket, address) = far_end.bind_udp().expect("binding");
        UdpSocket::bind("127.0.0.1:0")
            .expect("sender")
            .send_to(b"GET / HTTP/1.1\r\n\r\n", &address)
            .expect("junk");
        let refused = far_end.receive_datagram(&socket).expect_err("refused");
        assert!(!refused.retryable);
        assert!(!node().send("http://x", b"").expect_err("scheme").retryable);
        let (group, port) = multicast_group(GROUP).expect("multicast");
        assert_eq!(group, Ipv4Addr::new(239, 255, 255, 250));
        assert_eq!(port, "0.0.0.0:1900");
        assert!(multicast_group("127.0.0.1:1900").is_none());
        assert!(multicast_group("nonsense").is_none());
        assert!(node().claims().is_none());
        assert_eq!(node().name(), "ssdp");
        assert!(node().directions().receives());
    }
}
