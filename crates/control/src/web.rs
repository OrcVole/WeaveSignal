// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The public pages (`apps/web`'s build): the player with call-in at `/`, the invite page at
//! `/join/`, the echo test at `/echo-test/`, served by the control plane so they exist outside
//! the test stack (M3's known fault). Set `web.dir` to the build's directory; left empty,
//! nothing is served but the API.
//!
//! **What is served.** The directory is read once at start into memory: a fixed map from path
//! to file, so no request can name a file outside it, and a changed build needs a restart (a
//! release replaces the image anyway). Text files are also kept gzipped when that is smaller.
//!
//! **Caching.** Vite names everything under `/assets/` by its content, so those are
//! `public, max-age=31536000, immutable`; the pages themselves are `no-cache` (revalidated
//! with their ETag each time), so a new release reaches a phone on its next load.
//!
//! **Headers.** Every response carries a Content-Security-Policy that allows only the
//! station's own scripts, styles, workers and API, plus the two other origins the pages talk
//! to: LiveKit's (`room.public_url`, signalling) and the listener stream's (`join.stream_url`);
//! no inline script, no eval, no framing. Inline `style` attributes are allowed
//! (`style-src-attr`): the shared glyph sprite carries one, and a style attribute cannot run
//! script. The microphone and the camera are allowed for this origin only (`Permissions-Policy`):
//! guests, DJs and presenters send their camera from the join page (M5).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// The longest a build may be, all files together: the pages are about 1 MB.
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

struct File {
    body: Bytes,
    gzip: Option<Bytes>,
    content_type: &'static str,
    etag: String,
    immutable: bool,
}

/// The public pages, in memory, with the headers every response carries.
pub struct Site {
    files: BTreeMap<String, File>,
    csp: HeaderValue,
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn compressible(ct: &str) -> bool {
    ct.starts_with("text/") || ct.contains("json") || ct.contains("svg")
}

fn gzip(data: &[u8]) -> Option<Vec<u8>> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(data).ok()?;
    let out = enc.finish().ok()?;
    (out.len() + 64 < data.len()).then_some(out)
}

/// Pages and routes no search engine should list, even on a listed station: the API, the
/// invite page (its address is a secret) and the echo test.
fn never_listed(path: &str) -> bool {
    // The open routes for a station's website follow the listing setting (ADR-0195).
    if crate::api::is_open(path) {
        return false;
    }
    ["/api", "/join", "/echo-test"].iter().any(|p| {
        path == *p
            || path
                .strip_prefix(p)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

const NOINDEX_META: &str = "<meta name=\"robots\" content=\"noindex, nofollow\">";

/// A page with the robots meta tag added at the end of its head; a page without a head is
/// returned as it was (the `X-Robots-Tag` header still says it).
fn with_noindex(page: Vec<u8>) -> Vec<u8> {
    let Some(at) = page
        .windows(7)
        .position(|w| w.eq_ignore_ascii_case(b"</head>"))
    else {
        return page;
    };
    let mut out = Vec::with_capacity(page.len() + NOINDEX_META.len());
    out.extend_from_slice(&page[..at]);
    out.extend_from_slice(NOINDEX_META.as_bytes());
    out.extend_from_slice(&page[at..]);
    out
}

/// Search-engine listing (ADR-0133), for every response the control plane gives: unless the
/// station is `listed`, all of them say `X-Robots-Tag: noindex, nofollow` and `/robots.txt`
/// disallows everything. A listed station still says it of the API, the invite page and the
/// echo test, except the API's open routes for a station's website (`/api/v1/public/…` and
/// now playing, ADR-0195), which follow the setting. A header already set (by a proxy in front, or a route) is left alone.
pub async fn robots(
    axum::extract::State(listed): axum::extract::State<bool>,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_owned();
    if path == "/robots.txt" && matches!(*req.method(), Method::GET | Method::HEAD) {
        let body = if listed {
            "User-agent: *\nAllow: /api/v1/public/\nAllow: /api/v1/now-playing\nDisallow: /api/\nDisallow: /join/\nDisallow: /echo-test/\n"
        } else {
            "User-agent: *\nDisallow: /\n"
        };
        let mut res = (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            body,
        )
            .into_response();
        if *req.method() == Method::HEAD {
            *res.body_mut() = Body::empty();
        }
        return res;
    }
    let mut res = next.run(req).await;
    if !listed || never_listed(&path) {
        res.headers_mut()
            .entry(HeaderName::from_static("x-robots-tag"))
            .or_insert(HeaderValue::from_static("noindex, nofollow"));
    }
    res
}

/// The origin (`scheme://host[:port]`) of an absolute URL, or `None` for a relative one.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+'))
    .then(|| format!("{}://{host}", scheme.to_ascii_lowercase()))
}

/// The Content-Security-Policy for the pages, given LiveKit's public URL and the stream's.
pub fn csp(room_public_url: &str, stream_url: &str) -> String {
    csp_with_hls(room_public_url, stream_url, "")
}

/// The same, with the desk's HLS (M5): the player fetches its playlists and segments
/// (`connect-src`, when on another origin, an edge or a CDN) and plays them through Media
/// Source Extensions, whose media is a `blob:` URL (`media-src`).
pub fn csp_with_hls(room_public_url: &str, stream_url: &str, hls_url: &str) -> String {
    let mut connect = vec!["'self'".to_owned()];
    if let Some(o) = origin(hls_url) {
        connect.push(o);
    }
    if let Some(o) = origin(room_public_url) {
        // Signalling is WebSocket; the client also checks the same origin over HTTP when a
        // connection fails.
        let http = o
            .replacen("wss://", "https://", 1)
            .replacen("ws://", "http://", 1);
        connect.push(o.clone());
        if http != o {
            connect.push(http);
        }
    }
    let mut media = vec!["'self'".to_owned(), "blob:".to_owned()];
    if let Some(o) = origin(stream_url) {
        media.push(o);
    }
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; \
         img-src 'self' data:; font-src 'self'; connect-src {}; media-src {}; worker-src 'self'; \
         manifest-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'",
        connect.join(" "),
        media.join(" ")
    )
}

impl Site {
    /// Read the build at `dir` into memory, every page marked `noindex` (the default,
    /// ADR-0133).
    pub fn load(dir: &Path, csp: &str) -> anyhow::Result<Self> {
        Self::load_listed(dir, csp, false)
    }

    /// Read the build at `dir` into memory. Pages carry a `noindex` robots meta tag unless the
    /// station is `listed` (ADR-0133); the invite page and the echo test carry it always.
    pub fn load_listed(dir: &Path, csp: &str, listed: bool) -> anyhow::Result<Self> {
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        let mut stack = vec![PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for entry in
                std::fs::read_dir(&d).with_context(|| format!("cannot read {}", d.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !kind.is_file() {
                    continue; // no links out of the build
                }
                let rel = path.strip_prefix(dir).expect("under the build");
                let Some(rel) = rel.to_str() else { continue };
                if rel.split('/').any(|p| p.starts_with('.')) {
                    continue;
                }
                let mut data = std::fs::read(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?;
                if rel.ends_with(".html") && (!listed || never_listed(&format!("/{rel}"))) {
                    data = with_noindex(data);
                }
                total += data.len() as u64;
                anyhow::ensure!(
                    total <= MAX_TOTAL,
                    "{} is larger than a build of the pages should be",
                    dir.display()
                );
                let ct = content_type(&path);
                let digest = Sha256::digest(&data);
                let etag = format!(
                    "\"{}\"",
                    digest
                        .iter()
                        .take(12)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                );
                let gz = compressible(ct).then(|| gzip(&data)).flatten();
                let file = File {
                    body: data.into(),
                    gzip: gz.map(Into::into),
                    content_type: ct,
                    etag,
                    immutable: rel.starts_with("assets/"),
                };
                files.insert(format!("/{rel}"), file);
            }
        }
        anyhow::ensure!(
            files.contains_key("/index.html"),
            "{} has no index.html: build the pages (`just web-check`) and point `web.dir` at apps/web/dist",
            dir.display()
        );
        Ok(Self {
            files,
            csp: HeaderValue::from_str(csp).context("the CSP is not a header value")?,
        })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn find(&self, path: &str) -> Option<&File> {
        if let Some(f) = self.files.get(path) {
            return Some(f);
        }
        // `/join/` and `/join` are the join page; `/` is the player.
        let dir = path.trim_end_matches('/');
        self.files.get(&format!("{dir}/index.html"))
    }

    fn headers(&self, h: &mut HeaderMap) {
        h.insert(header::CONTENT_SECURITY_POLICY, self.csp.clone());
        h.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        h.insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        h.insert(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        );
        h.insert(
            HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("same-origin"),
        );
        h.insert(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "microphone=(self), camera=(self), geolocation=(), payment=(), usb=(), interest-cohort=()",
            ),
        );
    }

    /// Answer one request that no API route took.
    pub fn respond(&self, req: &Request) -> Response {
        let path = req.uri().path();
        if path.starts_with("/api/") || path == "/api" {
            return crate::app::problem(StatusCode::NOT_FOUND, "no such route");
        }
        let mut res = if !matches!(*req.method(), Method::GET | Method::HEAD) {
            let mut r = (StatusCode::METHOD_NOT_ALLOWED, "GET or HEAD only\n").into_response();
            r.headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            r
        } else if let Some(f) = self.find(path) {
            self.file(req, f)
        } else {
            (StatusCode::NOT_FOUND, "Not found\n").into_response()
        };
        self.headers(res.headers_mut());
        res
    }

    fn file(&self, req: &Request, f: &File) -> Response {
        let mut h = HeaderMap::new();
        h.insert(header::ETAG, HeaderValue::from_str(&f.etag).expect("hex"));
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(if f.immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            }),
        );
        if f.gzip.is_some() {
            h.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        }
        let fresh = req
            .headers()
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|t| t.trim() == f.etag || t.trim() == "*"));
        if fresh {
            return (StatusCode::NOT_MODIFIED, h).into_response();
        }
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(f.content_type),
        );
        let wants_gzip = req
            .headers()
            .get(header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|e| e.trim().split(';').next() == Some("gzip") && !e.contains("q=0"))
            });
        let body = match (&f.gzip, wants_gzip) {
            (Some(gz), true) => {
                h.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
                gz.clone()
            }
            _ => f.body.clone(),
        };
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
        let body = if *req.method() == Method::HEAD {
            Body::empty()
        } else {
            Body::from(body)
        };
        (StatusCode::OK, h, body).into_response()
    }
}

/// The router's fallback: the pages when a build is loaded.
pub fn fallback(site: Arc<Site>) -> impl Fn(Request) -> std::future::Ready<Response> + Clone {
    move |req: Request| std::future::ready(site.respond(&req))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn build() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("weavesignal-web-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::create_dir_all(dir.join("join")).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>Listen</title>",
        )
        .unwrap();
        std::fs::write(
            dir.join("join/index.html"),
            "<!doctype html><title>Join</title>",
        )
        .unwrap();
        std::fs::write(
            dir.join("assets/player-abc123.js"),
            "console.log('x');".repeat(200),
        )
        .unwrap();
        std::fs::write(dir.join(".hidden"), "no").unwrap();
        dir
    }

    fn app(dir: &Path) -> Router {
        let site = Arc::new(
            Site::load(
                dir,
                &csp(
                    "wss://live.example.org",
                    "https://radio.example.org/programme.opus",
                ),
            )
            .unwrap(),
        );
        Router::new()
            .route("/api/v1/x", axum::routing::get(|| async { "api" }))
            .fallback(fallback(site))
    }

    async fn get(app: &Router, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut req = Request::get(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        app.clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn pages_revalidate_assets_are_immutable_and_the_api_wins() {
        let dir = build();
        let app = app(&dir);
        let r = get(&app, "/", &[]).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["cache-control"], "no-cache");
        assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
        let csp = r.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(csp.contains("script-src 'self';"), "{csp}");
        assert!(
            csp.contains("connect-src 'self' wss://live.example.org https://live.example.org;"),
            "{csp}"
        );
        assert!(
            csp.contains("media-src 'self' blob: https://radio.example.org;"),
            "{csp}"
        );
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("unsafe-eval") && !csp.contains("script-src 'self' 'unsafe-inline'"));
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        // The join page sends a guest's camera as well as their voice (M5).
        let pp = r.headers()["permissions-policy"].to_str().unwrap();
        assert!(
            pp.contains("microphone=(self)") && pp.contains("camera=(self)"),
            "{pp}"
        );
        let etag = r.headers()["etag"].to_str().unwrap().to_owned();
        assert_eq!(
            get(&app, "/", &[("if-none-match", &etag)]).await.status(),
            StatusCode::NOT_MODIFIED
        );

        for p in ["/join/", "/join"] {
            let r = get(&app, p, &[]).await;
            let body = r.into_body().collect().await.unwrap().to_bytes();
            assert!(String::from_utf8_lossy(&body).contains("Join"), "{p}");
        }
        let r = get(
            &app,
            "/assets/player-abc123.js",
            &[("accept-encoding", "gzip, br")],
        )
        .await;
        assert_eq!(
            r.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(r.headers()["content-encoding"], "gzip");
        assert_eq!(
            r.headers()["content-type"],
            "text/javascript; charset=utf-8"
        );
        let r = get(&app, "/assets/player-abc123.js", &[]).await;
        assert!(r.headers().get("content-encoding").is_none());

        let r = get(&app, "/api/v1/x", &[]).await;
        assert_eq!(r.into_body().collect().await.unwrap().to_bytes(), "api");
        assert_eq!(
            get(&app, "/api/v1/nothing", &[]).await.status(),
            StatusCode::NOT_FOUND
        );
        for p in [
            "/.hidden",
            "/../Cargo.toml",
            "/assets/../../etc/passwd",
            "/nothing.html",
        ] {
            assert_eq!(
                get(&app, p, &[]).await.status(),
                StatusCode::NOT_FOUND,
                "{p}"
            );
        }
        let post = app
            .clone()
            .oneshot(Request::post("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn listing_app(listed: bool) -> (Router, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("weavesignal-web-{}", uuid::Uuid::new_v4().simple()));
        for sub in ["join", "echo-test"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
            std::fs::write(
                dir.join(sub).join("index.html"),
                "<!doctype html><head><title>x</title></head><body>x</body>",
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><head><title>Listen</title></head><body>x</body>",
        )
        .unwrap();
        let site = Arc::new(Site::load_listed(&dir, "default-src 'none'", listed).unwrap());
        let app = Router::new()
            .route("/api/v1/x", axum::routing::get(|| async { "api" }))
            .route("/feeds/a.xml", axum::routing::get(|| async { "<rss/>" }))
            .fallback(fallback(site))
            .layer(axum::middleware::from_fn_with_state(listed, robots));
        (app, dir)
    }

    async fn text(r: Response) -> String {
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn an_unlisted_station_says_noindex_everywhere() {
        let (app, dir) = listing_app(false);
        for p in ["/", "/join/", "/api/v1/x", "/feeds/a.xml", "/missing"] {
            let r = get(&app, p, &[]).await;
            assert_eq!(r.headers()["x-robots-tag"], "noindex, nofollow", "{p}");
        }
        for p in ["/", "/join/", "/echo-test/"] {
            let body = text(get(&app, p, &[]).await).await;
            assert!(body.contains(NOINDEX_META), "{p}: {body}");
        }
        let r = get(&app, "/robots.txt", &[]).await;
        assert_eq!(text(r).await, "User-agent: *\nDisallow: /\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_listed_station_lets_its_public_address_be_found_but_not_its_api_or_invites() {
        let (app, dir) = listing_app(true);
        for p in ["/", "/feeds/a.xml"] {
            let r = get(&app, p, &[]).await;
            assert!(r.headers().get("x-robots-tag").is_none(), "{p}");
        }
        assert!(!text(get(&app, "/", &[]).await).await.contains("noindex"));
        for p in ["/api/v1/x", "/join/", "/echo-test/"] {
            let r = get(&app, p, &[]).await;
            assert_eq!(r.headers()["x-robots-tag"], "noindex, nofollow", "{p}");
        }
        for p in ["/join/", "/echo-test/"] {
            assert!(text(get(&app, p, &[]).await).await.contains(NOINDEX_META));
        }
        let robots = text(get(&app, "/robots.txt", &[]).await).await;
        assert!(!robots.contains("Disallow: /\n"), "{robots}");
        assert!(robots.contains("Disallow: /api/"), "{robots}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_default_load_is_unlisted_and_never_listed_matches_whole_segments() {
        assert!(never_listed("/join/") && never_listed("/join") && never_listed("/api/v1/x"));
        assert!(!never_listed("/joined") && !never_listed("/"));
    }

    #[test]
    fn a_build_without_its_player_is_refused() {
        let dir =
            std::env::temp_dir().join(format!("weavesignal-web-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Site::load(&dir, "default-src 'none'").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relative_urls_add_no_origin() {
        let c = csp("", "/listen/programme.opus");
        assert!(
            c.contains("connect-src 'self';") && c.contains("media-src 'self' blob:;"),
            "{c}"
        );
        assert_eq!(
            origin("ws://127.0.0.1:39880/rtc").as_deref(),
            Some("ws://127.0.0.1:39880")
        );
    }
}
