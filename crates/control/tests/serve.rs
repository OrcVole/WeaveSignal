// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The binary end to end: it serves on its configured port, and refuses a taken one by name.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_weavesignal-control");
const STATION: &str = "3d0b7a52-1f7e-4a8b-9a51-0d2b6a4e9c11";

fn config_file(name: &str, control_port: u16) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.toml"));
    std::fs::write(
        &path,
        format!(
            "[station]\nid = \"{STATION}\"\nname = \"Kettle FM\"\n\n[ports]\ncontrol = {control_port}\n\n[log]\nformat = \"json\"\n"
        ),
    )
    .unwrap();
    path
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_get(port: u16, path: &str) -> Option<String> {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn serves_health_and_the_station_on_the_configured_port() {
    let port = free_port();
    let cfg = config_file("serve", port);
    let _child = Kill(
        Command::new(BIN)
            .args(["--config", cfg.to_str().unwrap(), "serve"])
            .env_remove("WEAVESIGNAL_DATABASE_URL")
            .env_remove("RUST_LOG")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let health = loop {
        if let Some(r) = http_get(port, "/health") {
            break r;
        }
        assert!(
            Instant::now() < deadline,
            "the control plane never answered"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains("\"status\":\"ok\""), "{health}");
    let station = http_get(port, "/api/v1/station").unwrap();
    assert!(station.contains(STATION), "{station}");
}

#[test]
fn a_taken_port_is_named_with_the_next_suggestion() {
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let cfg = config_file("taken", port);
    let out = Command::new(BIN)
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .env_remove("WEAVESIGNAL_DATABASE_URL")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("port {port}/tcp for `control`")),
        "{err}"
    );
    assert!(err.contains("is in use"), "{err}");
    assert!(err.contains("any free port may be used"), "{err}");
    drop(held);
}

#[test]
fn check_ports_reports_a_taken_port() {
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let cfg = config_file("check", port);
    let out = Command::new(BIN)
        .args(["--config", cfg.to_str().unwrap(), "check-ports"])
        .output()
        .unwrap();
    // Other defaults (443, 80, LiveKit's) may be taken or privileged on the test machine, so
    // only the held port is asserted.
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l.starts_with("TAKEN")
            && l.contains(&format!("{port}/tcp"))
            && l.contains("control")),
        "{stdout}"
    );
}

#[test]
fn sample_config_has_a_fresh_station_identifier() {
    let out = Command::new(BIN).arg("sample-config").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains("00000000-0000-0000-0000-000000000000"));
    assert!(text.contains("[ports]"));
}

#[test]
fn a_missing_settings_file_is_named() {
    let out = Command::new(BIN)
        .args(["--config", "/nonexistent/weavesignal.toml", "serve"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("/nonexistent/weavesignal.toml"));
}
