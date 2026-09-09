//! HTTPU, the HTTP-shaped text SSDP puts in a datagram: a start line that
//! is `NOTIFY * HTTP/1.1`, `M-SEARCH * HTTP/1.1` or `HTTP/1.1 200 OK`,
//! then headers to a blank line and nothing after it. Header names are
//! matched without regard to case, as HTTP's are, because every `UPnP`
//! stack capitalises them differently.

use transport::error::{Result, protocol_error};

/// The multicast group and port SSDP lives on.
pub const GROUP: &str = "239.255.255.250:1900";
/// `NTS` of a device arriving.
pub const ALIVE: &str = "ssdp:alive";
/// `NTS` of a device leaving.
pub const BYEBYE: &str = "ssdp:byebye";
/// `ST` that asks for everything.
pub const ALL: &str = "ssdp:all";

/// The three start lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Notify,
    Search,
    Response,
}

impl Kind {
    /// The start line as written.
    #[must_use]
    pub const fn start_line(self) -> &'static str {
        match self {
            Self::Notify => "NOTIFY * HTTP/1.1",
            Self::Search => "M-SEARCH * HTTP/1.1",
            Self::Response => "HTTP/1.1 200 OK",
        }
    }
}

/// One message: its kind and its headers in the order written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: Kind,
    pub headers: Vec<(String, String)>,
}

impl Message {
    #[must_use]
    pub const fn new(kind: Kind) -> Self {
        Self {
            kind,
            headers: Vec::new(),
        }
    }

    /// With `name: value` appended.
    #[must_use]
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// The first header called `name`, case not mattering.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// `NOTIFY` that `usn` of type `nt` is at `location`.
    #[must_use]
    pub fn alive(nt: &str, usn: &str, location: &str, server: &str) -> Self {
        Self::new(Kind::Notify)
            .with("HOST", GROUP)
            .with("CACHE-CONTROL", "max-age=1800")
            .with("LOCATION", location)
            .with("NT", nt)
            .with("NTS", ALIVE)
            .with("SERVER", server)
            .with("USN", usn)
    }

    /// `NOTIFY` that `usn` of type `nt` has gone.
    #[must_use]
    pub fn byebye(nt: &str, usn: &str) -> Self {
        Self::new(Kind::Notify)
            .with("HOST", GROUP)
            .with("NT", nt)
            .with("NTS", BYEBYE)
            .with("USN", usn)
    }

    /// `M-SEARCH` for `st`, answers spread over `mx` seconds.
    #[must_use]
    pub fn search(st: &str, mx: u8) -> Self {
        Self::new(Kind::Search)
            .with("HOST", GROUP)
            .with("MAN", "\"ssdp:discover\"")
            .with("MX", &mx.to_string())
            .with("ST", st)
    }

    /// The answer to a search: `usn` of type `st` is at `location`.
    #[must_use]
    pub fn response(st: &str, usn: &str, location: &str, server: &str) -> Self {
        Self::new(Kind::Response)
            .with("CACHE-CONTROL", "max-age=1800")
            .with("EXT", "")
            .with("LOCATION", location)
            .with("SERVER", server)
            .with("ST", st)
            .with("USN", usn)
    }

    /// The notification type: `NT` of a notify, `ST` of a search or
    /// response.
    #[must_use]
    pub fn notification_type(&self) -> &str {
        let name = if self.kind == Kind::Notify {
            "NT"
        } else {
            "ST"
        };
        self.header(name).unwrap_or("")
    }
}

/// `message` as the datagram.
#[must_use]
pub fn format(message: &Message) -> Vec<u8> {
    let mut out = String::from(message.kind.start_line());
    out.push_str("\r\n");
    for (name, value) in &message.headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// Whether `bytes` already open with one of the three start lines.
#[must_use]
pub fn is_message(bytes: &[u8]) -> bool {
    bytes.starts_with(b"NOTIFY ") || bytes.starts_with(b"M-SEARCH ") || bytes.starts_with(b"HTTP/")
}

/// One datagram.
///
/// # Errors
/// Not text, not one of the three start lines, or a header line without
/// a colon.
pub fn parse(bytes: &[u8]) -> Result<Message> {
    let text = std::str::from_utf8(bytes).map_err(|_| protocol_error("a message not UTF-8"))?;
    let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r'));
    let start = lines.next().unwrap_or("").trim();
    let kind = match start.split_whitespace().collect::<Vec<_>>()[..] {
        ["NOTIFY", "*", "HTTP/1.1"] => Kind::Notify,
        ["M-SEARCH", "*", "HTTP/1.1"] => Kind::Search,
        ["HTTP/1.1", "200", ..] => Kind::Response,
        _ => return Err(protocol_error(format!("not an SSDP start line: {start:?}"))),
    };
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| protocol_error(format!("a header without a colon: {line:?}")))?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    Ok(Message { kind, headers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_round_trips_and_headers_read_without_case() {
        let alive = Message::alive(
            "upnp:rootdevice",
            "uuid:1::upnp:rootdevice",
            "http://10.0.0.5/d.xml",
            "xmip/0.1",
        );
        let bytes = format(&alive);
        assert!(bytes.starts_with(b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n"));
        assert!(bytes.ends_with(b"USN: uuid:1::upnp:rootdevice\r\n\r\n"));
        assert!(is_message(&bytes));
        let back = parse(&bytes).expect("parse");
        assert_eq!(back, alive);
        assert_eq!(back.header("location"), Some("http://10.0.0.5/d.xml"));
        assert_eq!(back.header("Nts"), Some(ALIVE));
        assert_eq!(back.notification_type(), "upnp:rootdevice");
        let search = parse(&format(&Message::search(ALL, 2))).expect("search");
        assert_eq!(search.kind, Kind::Search);
        assert_eq!(search.header("MAN"), Some("\"ssdp:discover\""));
        assert_eq!(search.notification_type(), ALL);
        let response = parse(&format(&Message::response("st", "usn", "loc", "srv"))).expect("ok");
        assert_eq!(response.kind, Kind::Response);
        assert_eq!(response.header("EXT"), Some(""));
        assert_eq!(
            parse(&format(&Message::byebye("nt", "usn")))
                .expect("bye")
                .header("NTS"),
            Some(BYEBYE)
        );
        let loose = parse(b"HTTP/1.1 200 OK\nst:  a \nusn:b\n\nignored").expect("bare newlines");
        assert_eq!(loose.header("ST"), Some("a"));
        assert_eq!(loose.header("USN"), Some("b"));
    }

    #[test]
    fn what_is_not_httpu_is_refused() {
        assert!(parse(b"GET / HTTP/1.1\r\n\r\n").is_err(), "a request");
        assert!(parse(b"NOTIFY / HTTP/1.1\r\n\r\n").is_err(), "not *");
        assert!(parse(b"HTTP/1.1 404 Not Found\r\n\r\n").is_err(), "not 200");
        assert!(parse(b"NOTIFY * HTTP/1.1\r\nno colon\r\n\r\n").is_err());
        assert!(parse(&[0xff]).is_err(), "not text");
        assert!(
            !is_message(b"http://10.0.0.5/d.xml"),
            "a location is not a message"
        );
    }
}
