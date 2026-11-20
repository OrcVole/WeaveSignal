// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WHIP's HTTP side as a host's software speaks it (17d; RFC 9725): a `POST` of an SDP offer to
//! `/whip/{key id}` with the key's secret as the bearer (OBS's *Bearer Token*), answered `201
//! Created` with the SDP answer and the session's address in `Location`; a `DELETE` of that
//! address ends it. Trickle ICE (`PATCH`) is not offered: the station is ICE-lite with one host
//! candidate, so there is nothing to trickle.
//!
//! The control plane takes the request on the station's public domain and passes it on over
//! loopback, so only the request's method, path, headers and body are read here; the media goes
//! to [`crate::whip_mux`].

/// The longest offer taken: an OBS offer with audio and video is under 8 KB.
pub const MAX_BODY: usize = 32 * 1024;
/// The path's prefix.
pub const PREFIX: &str = "/whip/";

/// A request the ingest serves.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// A new session: the key id from the path, the secret from the bearer, the offer.
    Offer {
        key_id: String,
        secret: String,
        sdp: String,
    },
    /// The end of a session the host's software made.
    End {
        key_id: String,
        session: String,
        secret: String,
    },
    /// `OPTIONS`: what the endpoint takes.
    Options,
}

/// Why a request is refused, each with its status.
#[derive(Debug, PartialEq, Eq)]
pub enum Bad {
    /// Not a WHIP path (404).
    NotFound,
    /// A method this path does not take (405), with the methods it does.
    Method(&'static str),
    /// No bearer (401).
    Credentials,
    /// Not `application/sdp` (415).
    MediaType,
    /// An offer over [`MAX_BODY`] (413).
    TooLarge,
    /// An offer that is not text, or empty (400).
    Offer,
}

impl Bad {
    /// The refusal to send.
    pub fn response(&self) -> Response {
        match self {
            Bad::NotFound => refusal(404, "There is no WHIP endpoint at this address."),
            Bad::Method(allow) => {
                let mut r = refusal(405, "WHIP takes POST for an offer and DELETE to end.");
                r.headers.push(("Allow".into(), (*allow).into()));
                r
            }
            Bad::Credentials => {
                let mut r = refusal(
                    401,
                    "No key: put the key's secret in your software's Bearer Token.",
                );
                r.headers.push(("WWW-Authenticate".into(), "Bearer".into()));
                r
            }
            Bad::MediaType => refusal(415, "WHIP sends an SDP offer (application/sdp)."),
            Bad::TooLarge => refusal(413, "The offer is too large."),
            Bad::Offer => refusal(400, "The offer is empty or not text."),
        }
    }
}

/// An answer to send back: status, headers, body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// The answer as HTTP/1.1 on the wire, the connection closed after it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (n, v) in &self.headers {
            out.push_str(&format!("{n}: {v}\r\n"));
        }
        out.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.body.len()
        ));
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }

    /// A header's value, by name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// A header's value from a list, by name in any case.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

/// A key id or session as the path carries it: short, and letters and digits only.
fn word(s: &str) -> Option<String> {
    (!s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| s.to_owned())
}

/// The secret from `Authorization: Bearer …`. OBS's field is called a stream key elsewhere, so
/// the Icecast password's form (`<key id>-<secret>`) is taken as well.
fn bearer(headers: &[(String, String)], key_id: &str) -> Option<String> {
    let v = header(headers, "authorization")?;
    let (scheme, token) = v.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    let token = token
        .strip_prefix(key_id)
        .and_then(|t| t.strip_prefix('-'))
        .unwrap_or(token);
    (!token.is_empty() && token.len() <= 128 && token.chars().all(|c| c.is_ascii_graphic()))
        .then(|| token.to_owned())
}

/// Read a request: what it asks for, or why it is refused.
pub fn parse(
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Request, Bad> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let rest = path.strip_prefix(PREFIX).ok_or(Bad::NotFound)?;
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [key] => {
            let key_id = word(key).ok_or(Bad::NotFound)?;
            match method {
                "OPTIONS" => Ok(Request::Options),
                "POST" => {
                    let secret = bearer(headers, &key_id).ok_or(Bad::Credentials)?;
                    let ct = header(headers, "content-type").unwrap_or_default();
                    let ct = ct.split(';').next().unwrap_or_default().trim();
                    if !ct.eq_ignore_ascii_case("application/sdp") {
                        return Err(Bad::MediaType);
                    }
                    if body.len() > MAX_BODY {
                        return Err(Bad::TooLarge);
                    }
                    let sdp = std::str::from_utf8(body).map_err(|_| Bad::Offer)?;
                    if sdp.trim().is_empty() {
                        return Err(Bad::Offer);
                    }
                    Ok(Request::Offer {
                        key_id,
                        secret,
                        sdp: sdp.to_owned(),
                    })
                }
                _ => Err(Bad::Method("POST, OPTIONS")),
            }
        }
        [key, session] => {
            let key_id = word(key).ok_or(Bad::NotFound)?;
            let session = word(session).ok_or(Bad::NotFound)?;
            match method {
                "DELETE" => {
                    let secret = bearer(headers, &key_id).ok_or(Bad::Credentials)?;
                    Ok(Request::End {
                        key_id,
                        session,
                        secret,
                    })
                }
                // No trickle ICE and no ICE restart: a PATCH is refused (RFC 9725 section 4.3).
                _ => Err(Bad::Method("DELETE")),
            }
        }
        _ => Err(Bad::NotFound),
    }
}

/// `201 Created` with the answer, the session's address and its ETag.
pub fn created(answer: &str, location: &str) -> Response {
    Response {
        status: 201,
        headers: vec![
            ("Content-Type".into(), "application/sdp".into()),
            ("Location".into(), location.into()),
        ],
        body: answer.as_bytes().to_vec(),
    }
}

/// `OPTIONS`: an offer is taken; no ICE servers are offered (no STUN, no TURN; ADR-0290).
pub fn options() -> Response {
    Response {
        status: 204,
        headers: vec![
            ("Accept-Post".into(), "application/sdp".into()),
            ("Allow".into(), "POST, OPTIONS".into()),
        ],
        body: Vec::new(),
    }
}

/// `200 OK` for a session ended.
pub fn ended() -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

/// A refusal with a sentence for the host, as plain text.
pub fn refusal(status: u16, why: &str) -> Response {
    Response {
        status,
        headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
        body: format!("{why}\n").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    const OFFER: &[u8] = b"v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";

    #[test]
    fn a_good_offer() {
        let r = parse(
            "POST",
            "/whip/abcd2345",
            &h(&[
                ("Authorization", "Bearer SECRETSECRET"),
                ("Content-Type", "application/sdp"),
            ]),
            OFFER,
        )
        .unwrap();
        assert_eq!(
            r,
            Request::Offer {
                key_id: "abcd2345".into(),
                secret: "SECRETSECRET".into(),
                sdp: String::from_utf8(OFFER.to_vec()).unwrap(),
            }
        );
    }

    #[test]
    fn the_icecast_passwords_form_is_taken_as_the_bearer() {
        let r = parse(
            "POST",
            "/whip/abcd2345?x=1",
            &h(&[
                ("authorization", "bearer abcd2345-SECRET"),
                ("content-type", "application/sdp; charset=utf-8"),
            ]),
            OFFER,
        )
        .unwrap();
        assert!(matches!(r, Request::Offer { secret, .. } if secret == "SECRET"));
    }

    #[test]
    fn a_missing_bearer_is_401() {
        let e = parse(
            "POST",
            "/whip/abcd2345",
            &h(&[("Content-Type", "application/sdp")]),
            OFFER,
        )
        .unwrap_err();
        assert_eq!(e, Bad::Credentials);
        assert_eq!(e.response().status, 401);
        assert_eq!(e.response().header("www-authenticate"), Some("Bearer"));
        let basic = parse(
            "POST",
            "/whip/abcd2345",
            &h(&[
                ("Authorization", "Basic Zm9vOmJhcg=="),
                ("Content-Type", "application/sdp"),
            ]),
            OFFER,
        );
        assert_eq!(basic, Err(Bad::Credentials));
    }

    #[test]
    fn each_refusal() {
        let auth = ("Authorization", "Bearer S");
        assert_eq!(parse("POST", "/other", &[], OFFER), Err(Bad::NotFound));
        assert_eq!(parse("POST", "/whip/", &[], OFFER), Err(Bad::NotFound));
        assert_eq!(parse("POST", "/whip/a/b/c", &[], OFFER), Err(Bad::NotFound));
        assert_eq!(
            parse("POST", "/whip/ab..cd", &[], OFFER),
            Err(Bad::NotFound)
        );
        assert_eq!(
            parse("GET", "/whip/abcd2345", &[], OFFER),
            Err(Bad::Method("POST, OPTIONS"))
        );
        assert_eq!(
            parse("PATCH", "/whip/abcd2345/s1", &h(&[auth]), b""),
            Err(Bad::Method("DELETE"))
        );
        assert_eq!(
            parse(
                "POST",
                "/whip/abcd2345",
                &h(&[auth, ("Content-Type", "application/json")]),
                OFFER
            ),
            Err(Bad::MediaType)
        );
        let big = vec![b'a'; MAX_BODY + 1];
        assert_eq!(
            parse(
                "POST",
                "/whip/abcd2345",
                &h(&[auth, ("Content-Type", "application/sdp")]),
                &big
            ),
            Err(Bad::TooLarge)
        );
        assert_eq!(
            parse(
                "POST",
                "/whip/abcd2345",
                &h(&[auth, ("Content-Type", "application/sdp")]),
                b" \r\n"
            ),
            Err(Bad::Offer)
        );
        assert_eq!(
            parse(
                "POST",
                "/whip/abcd2345",
                &h(&[auth, ("Content-Type", "application/sdp")]),
                &[0xff, 0xfe]
            ),
            Err(Bad::Offer)
        );
        for b in [
            Bad::NotFound,
            Bad::Method("DELETE"),
            Bad::Credentials,
            Bad::MediaType,
            Bad::TooLarge,
            Bad::Offer,
        ] {
            let r = b.response();
            assert!((400..500).contains(&r.status), "{b:?}");
            assert!(!r.body.is_empty());
        }
    }

    #[test]
    fn delete_ends_a_session_with_its_bearer_and_options_says_what_is_taken() {
        assert_eq!(
            parse(
                "DELETE",
                "/whip/abcd2345/s0me5ession",
                &h(&[("Authorization", "Bearer S")]),
                b""
            ),
            Ok(Request::End {
                key_id: "abcd2345".into(),
                session: "s0me5ession".into(),
                secret: "S".into()
            })
        );
        assert_eq!(
            parse("DELETE", "/whip/abcd2345/s1", &[], b""),
            Err(Bad::Credentials)
        );
        assert_eq!(
            parse("OPTIONS", "/whip/abcd2345", &[], b""),
            Ok(Request::Options)
        );
        let o = options();
        assert_eq!(o.status, 204);
        assert_eq!(o.header("accept-post"), Some("application/sdp"));
        assert!(o.header("link").is_none(), "no ICE servers are offered");
    }

    #[test]
    fn created_carries_the_answer_and_its_address() {
        let r = created("v=0\r\n", "/whip/abcd2345/s1");
        assert_eq!(r.status, 201);
        assert_eq!(r.header("content-type"), Some("application/sdp"));
        assert_eq!(r.header("location"), Some("/whip/abcd2345/s1"));
        let wire = String::from_utf8(r.to_bytes()).unwrap();
        assert!(wire.starts_with("HTTP/1.1 201 Created\r\n"), "{wire}");
        assert!(wire.contains("Content-Length: 5\r\n"));
        assert!(wire.ends_with("\r\n\r\nv=0\r\n"));
    }
}
