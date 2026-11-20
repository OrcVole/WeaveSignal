// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A pull end to end inside the ingest (PLAN 17b), with no container: a stand-in control plane
//! that names one pull, a stand-in Icecast mount that serves a real MP3, and a stand-in desk. The
//! supervisor must open the pull, admit it with way `pull`, run the decoding worker (this crate's
//! own binary), report it standing by, and hand 48 kHz float to the desk with the source's
//! identity and way. Every stand-in stops itself when the test is done.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use weavesignal_ingest::supervisor::{Settings, run};

const KEY: &str = "pullaaaa";

/// Accept connections until `stop`, each handled by `each` on this thread.
fn serve_until(
    stop: Arc<AtomicBool>,
    mut each: impl FnMut(TcpStream) + Send + 'static,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    each(s);
                }
                Err(_) => thread::sleep(Duration::from_millis(20)),
            }
        }
    });
    (addr, handle)
}

/// A request's head and body (by its Content-Length).
fn request(s: &mut TcpStream) -> (String, Vec<u8>) {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap_or(0) == 1 {
        head.push(b[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let len = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    let _ = s.read_exact(&mut body);
    (head, body)
}

/// A JSON body without its layout, to look for `"way":"pull"` in.
fn compact(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned())
}

#[test]
fn a_due_pull_is_opened_admitted_decoded_and_handed_to_the_desk() {
    let stop = Arc::new(AtomicBool::new(false));
    let mp3 = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone-44k.mp3"
    ))
    .unwrap();

    // The partner's mount, live: the file five times over, a little at a time as a server sends
    // a stream, then closed (the ingest opens it again after 1 s).
    let (mount, mount_thread) = {
        let stop = stop.clone();
        serve_until(stop.clone(), move |mut s| {
            let _ = request(&mut s);
            let _ = s.write_all(b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n");
            for _ in 0..5 {
                for chunk in mp3.chunks(4096) {
                    if stop.load(Ordering::Relaxed) || s.write_all(chunk).is_err() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        })
    };

    // The desk: takes the source's head, answers 200, counts what follows.
    let desk_heads = Arc::new(Mutex::new(Vec::<String>::new()));
    let desk_bytes = Arc::new(Mutex::new(0usize));
    let (desk, desk_thread) = {
        let (heads, bytes) = (desk_heads.clone(), desk_bytes.clone());
        serve_until(stop.clone(), move |mut s| {
            let (head, _) = request(&mut s);
            heads.lock().unwrap().push(head);
            let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\n");
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                *bytes.lock().unwrap() += n;
            }
        })
    };

    // The control plane: names the pull, admits it, takes reports and events.
    let calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let (control, control_thread) = {
        let calls = calls.clone();
        serve_until(stop.clone(), move |mut s| {
            let (head, body) = request(&mut s);
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            calls.lock().unwrap().push((path.clone(), compact(&body)));
            let json = match path.as_str() {
                "/api/v1/internal/sources/config" => format!(
                    r#"{{"listen":false,"port_icecast":0,"port_srt":0,"test_max_ms":900000,"pulls":[{{"key_id":"{KEY}","url":"http://{mount}/live","allow_private":true}}]}}"#
                ),
                "/api/v1/internal/sources/admit" => format!(
                    r#"{{"decision":"standby","identity":"source-{KEY}","name":"Elspeth Gordon","show":"steeped"}}"#
                ),
                "/api/v1/internal/sources/report" => r#"{"close":[],"to_test":[]}"#.to_owned(),
                _ => {
                    let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                    return;
                }
            };
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                )
                .as_bytes(),
            );
        })
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.spawn(run(Settings {
        control: format!("http://{control}"),
        token: "t".repeat(32),
        desk,
        desk_password: "desk".into(),
        bind: "127.0.0.1".parse().unwrap(),
        exe: env!("CARGO_BIN_EXE_weavesignal-ingest").into(),
        // WHIP stays off (no public address, and the control plane names no WHIP port), so
        // nothing listens here.
        whip_http: "127.0.0.1:9".parse().unwrap(),
        public_ip: None,
    }));

    // A second of 48 kHz stereo float is 384,000 bytes; the fixture is about a second and a half.
    let deadline = Instant::now() + Duration::from_secs(20);
    let reported = |calls: &[(String, String)]| {
        calls.iter().any(|(p, b)| {
            p.ends_with("/report") && b.contains(KEY) && b.contains(r#""way":"pull""#)
        })
    };
    while Instant::now() < deadline
        && !(*desk_bytes.lock().unwrap() > 200_000 && reported(&calls.lock().unwrap()))
    {
        thread::sleep(Duration::from_millis(100));
    }

    rt.shutdown_timeout(Duration::from_secs(2));
    stop.store(true, Ordering::Relaxed);
    for t in [mount_thread, desk_thread, control_thread] {
        t.join().unwrap();
    }

    let calls = calls.lock().unwrap();
    let admit = calls
        .iter()
        .find(|(p, _)| p.ends_with("/admit"))
        .expect("the pull was admitted");
    assert!(admit.1.contains(r#""way":"pull""#), "{}", admit.1);
    assert!(admit.1.contains(KEY));
    assert!(
        !admit.1.contains("secret"),
        "a pull gives no secret: {}",
        admit.1
    );
    assert!(
        reported(&calls),
        "the report names the pull standing by: {:?}, desk {} bytes, {:?}",
        calls
            .iter()
            .filter(|(p, _)| !p.ends_with("/config"))
            .take(12)
            .collect::<Vec<_>>(),
        desk_bytes.lock().unwrap(),
        desk_heads.lock().unwrap()
    );
    let heads = desk_heads.lock().unwrap();
    let head = heads.first().expect("the desk was handed the source");
    assert!(
        head.starts_with(&format!("PUT /sources/source-{KEY} ")),
        "{head}"
    );
    assert!(head.contains("X-Source-Way: pull"), "{head}");
    let bytes = *desk_bytes.lock().unwrap();
    assert!(bytes > 200_000, "the desk had {bytes} bytes of sound");
}
