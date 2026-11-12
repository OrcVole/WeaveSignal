// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat against a throwaway PostgreSQL, over real sockets: sign-in by email code, posting
//! and the rules, the hold and its release, reports and notices, hide with a statement of
//! reasons, a time-out, a ban taking past messages out, the private lane to the studio,
//! recovery after a reconnect, erasure, and the retention sweep with an evidence hold.
//!
//! Ignored by default; `just test-chat-db` runs them with `WEAVESIGNAL_TEST_DATABASE_URL` set.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use weavesignal_chat::config::{Config, MailMode, SAMPLE};
use weavesignal_chat::key::EmailKey;

const ADMIN: &str = "chat-test-admin-only";
const KEY: &str = "chat-test-email-key-0123456789abcdef";

struct Server {
    base: String,
    ws: String,
    outbox: PathBuf,
    db: sqlx::PgPool,
    station: uuid::Uuid,
    /// Where this chat keeps uploaded pictures (ADR-0235).
    avatars: PathBuf,
    chat: std::sync::Arc<weavesignal_chat::service::Chat>,
}

async fn server() -> Server {
    server_for(uuid::Uuid::new_v4(), "").await
}

/// A chat for `station`, passing the room's modes on to the control plane at `control` (none
/// when empty).
async fn server_for(station: uuid::Uuid, control: &str) -> Server {
    server_with(station, control, |_| {}).await
}

/// As [`server_for`], with the settings changed first.
async fn server_with(station: uuid::Uuid, control: &str, f: impl FnOnce(&mut Config)) -> Server {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let outbox = std::env::temp_dir().join(format!("weavesignal-chat-test-{station}"));
    let mut c = Config::from_toml(
        &SAMPLE.replace("00000000-0000-0000-0000-000000000000", &station.to_string()),
        "test",
    )
    .unwrap();
    c.control.url = control.to_owned();
    c.mail.mode = MailMode::Outbox;
    c.mail.outbox_dir = outbox.clone();
    c.database_url = Some(url.clone());
    c.admin_token = Some(ADMIN.into());
    c.email_key = Some(EmailKey::parse(KEY).unwrap());
    // The media worker is the chat's own binary; under `cargo test` this program is the test.
    let avatars = std::env::temp_dir().join(format!("weavesignal-chat-avatars-{station}"));
    c.avatars.dir = avatars.clone();
    c.avatars.worker = Some(env!("CARGO_BIN_EXE_weavesignal-chat").into());
    f(&mut c);
    let state = weavesignal_chat::start(c).await.unwrap();
    let chat = state.chat.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app =
        weavesignal_chat::http::router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let db = sqlx::PgPool::connect(&url).await.unwrap();
    Server {
        base: format!("http://{addr}"),
        ws: format!("ws://{addr}/api/v1/chat/socket"),
        outbox,
        db,
        station,
        avatars,
        chat,
    }
}

async fn http(
    method: &str,
    url: String,
    bearer: Option<String>,
    body: Option<Value>,
) -> (u16, Value) {
    let method = method.to_owned();
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let auth = bearer.map(|b| format!("Bearer {b}"));
        let mut resp = match (method.as_str(), body) {
            ("GET", _) => {
                let mut r = agent.get(&url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.call().unwrap()
            }
            ("DELETE", _) => {
                let mut r = agent.delete(&url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.call().unwrap()
            }
            ("PUT", body) => {
                let mut r = agent.put(&url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.send_json(body.unwrap_or(json!({}))).unwrap()
            }
            (_, body) => {
                let mut r = agent.post(&url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.send_json(body.unwrap_or(json!({}))).unwrap()
            }
        };
        let code = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        (code, serde_json::from_str(&text).unwrap_or(Value::Null))
    })
    .await
    .unwrap()
}

impl Server {
    async fn get(&self, path: &str, bearer: Option<&str>) -> (u16, Value) {
        http(
            "GET",
            format!("{}{path}", self.base),
            bearer.map(str::to_owned),
            None,
        )
        .await
    }
    async fn post(&self, path: &str, bearer: Option<&str>, body: Value) -> (u16, Value) {
        http(
            "POST",
            format!("{}{path}", self.base),
            bearer.map(str::to_owned),
            Some(body),
        )
        .await
    }

    /// The newest code in the outbox for an address.
    fn code_for(&self, email: &str) -> String {
        let mut newest: Option<(std::time::SystemTime, String)> = None;
        for e in std::fs::read_dir(&self.outbox).unwrap() {
            let p = e.unwrap().path();
            let text = std::fs::read_to_string(&p).unwrap();
            if !text.contains(&format!("To: {email}")) {
                continue;
            }
            let t = std::fs::metadata(&p).unwrap().modified().unwrap();
            let code = text
                .split("chat is ")
                .nth(1)
                .unwrap()
                .chars()
                .take(6)
                .collect::<String>();
            if newest.as_ref().is_none_or(|(n, _)| t >= *n) {
                newest = Some((t, code));
            }
        }
        newest.expect("a code in the outbox").1
    }

    /// Sign a new chatter in and make their account old enough to post.
    async fn chatter(&self, email: &str, name: &str) -> (String, String) {
        let (code, _) = self
            .post("/api/v1/chat/signin/start", None, json!({"email": email}))
            .await;
        assert_eq!(code, 202);
        let c = self.code_for(email);
        let (_, info) = self.get("/api/v1/chat/info", None).await;
        let version = info["terms_version"].as_str().unwrap().to_owned();
        let (code, body) = self
            .post(
                "/api/v1/chat/signin/finish",
                None,
                json!({"email": email, "code": c}),
            )
            .await;
        assert_eq!(code, 422, "{body}");
        assert_eq!(body["needs"], json!(["name", "age_ok", "terms"]));
        let (code, body) = self
            .post(
                "/api/v1/chat/signin/finish",
                None,
                json!({"email": email, "code": c, "name": name, "age_ok": true, "terms_version": version}),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let id = body["me"]["id"].as_str().unwrap().to_owned();
        sqlx::query(
            "UPDATE chat_account SET created_at = now() - interval '3 days' WHERE id = $1::uuid",
        )
        .bind(&id)
        .execute(&self.db)
        .await
        .unwrap();
        (body["session"].as_str().unwrap().to_owned(), id)
    }

    async fn staff(&self, name: &str) -> String {
        let (code, body) = self
            .post(
                "/api/v1/chat/staff/session",
                Some(ADMIN),
                json!({"name": name}),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        body["session"].as_str().unwrap().to_owned()
    }

    async fn moderate(&self, cmd: Value) -> (u16, Value) {
        self.post("/api/v1/chat/mod", Some(ADMIN), cmd).await
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Client {
    ws: Ws,
    seen: Vec<Value>,
}

impl Client {
    async fn open(s: &Server, hello: Value) -> (Client, Value) {
        let (mut ws, _) = tokio_tungstenite::connect_async(&s.ws).await.unwrap();
        ws.send(Message::Text(hello.to_string().into()))
            .await
            .unwrap();
        let mut c = Client { ws, seen: vec![] };
        let w = c.until(|f| f["t"] == "welcome").await;
        (c, w)
    }

    async fn send(&mut self, v: Value) {
        self.ws
            .send(Message::Text(v.to_string().into()))
            .await
            .unwrap();
    }

    /// The next frame that matches, within 5 s; earlier frames are kept in `seen`.
    async fn until(&mut self, want: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.seen.iter().position(&want) {
            return self.seen.remove(i);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let next = tokio::time::timeout(left, self.ws.next())
                .await
                .unwrap_or_else(|_| panic!("timed out; seen {:?}", self.seen));
            match next {
                Some(Ok(Message::Text(t))) => {
                    let v: Value = serde_json::from_str(&t).unwrap();
                    if want(&v) {
                        return v;
                    }
                    self.seen.push(v);
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    }

    /// Whether a matching frame arrives within `ms`.
    async fn gets(&mut self, want: impl Fn(&Value) -> bool, ms: u64) -> bool {
        if self.seen.iter().any(&want) {
            return true;
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(left, self.ws.next()).await {
                Err(_) => return false,
                Ok(Some(Ok(Message::Text(t)))) => {
                    let v: Value = serde_json::from_str(&t).unwrap();
                    if want(&v) {
                        return true;
                    }
                    self.seen.push(v);
                }
                Ok(Some(Ok(_))) => {}
                Ok(_) => return false,
            }
        }
    }

    async fn post(&mut self, nonce: &str, kind: &str, body: &str) -> Value {
        self.send(json!({"t": "post", "nonce": nonce, "kind": kind, "body": body, "playhead_ms": now() - 15_000}))
            .await;
        let n = nonce.to_owned();
        self.until(move |f| f["t"] == "ack" && f["nonce"] == n.as_str())
            .await
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn sign_in_post_report_hide_with_a_statement_time_out_and_ban() {
    let s = server().await;
    let (alastair, alastair_id) = s
        .chatter("alastair.buchanan@mail.example", "Alastair Buchanan")
        .await;
    let (jessie, _) = s.chatter("jessie.brown@mail.example", "Jessie Brown").await;
    let elspeth = s.staff("Elspeth Gordon").await;

    // A reader without an account, a chatter, another chatter, the screener.
    let (mut reader, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert_eq!(w["me"], Value::Null);
    assert_eq!(w["rules"]["links_trusted"], true, "ADR-0036's default");
    assert_eq!(w["moderation"], Value::Null);
    let (mut a, w) = Client::open(&s, json!({"t": "hello", "session": alastair})).await;
    assert_eq!(w["me"]["name"], "Alastair Buchanan");
    let (mut j, _) = Client::open(&s, json!({"t": "hello", "session": jessie})).await;
    let (mut mods, w) = Client::open(&s, json!({"t": "hello", "session": elspeth})).await;
    assert!(w["moderation"]["items"].is_array());

    // Reading is open; posting without an account is refused plainly.
    reader
        .send(json!({"t": "post", "nonce": "r1", "kind": "chat", "body": "hello"}))
        .await;
    let ack = reader.until(|f| f["t"] == "ack").await;
    assert_eq!(ack["state"], "refused");
    assert_eq!(ack["code"], "signed_out");

    // A post reaches everyone with both timestamps.
    let ack = a.post("a1", "chat", "Drove the N22 for nine years").await;
    assert_eq!(ack["state"], "visible", "{ack}");
    let id = ack["id"].as_i64().unwrap();
    let m = reader.until(|f| f["t"] == "msg").await;
    assert_eq!(m["m"]["id"], id);
    assert_eq!(m["m"]["author"]["name"], "Alastair Buchanan");
    assert!(m["m"]["server_ms"].as_i64().unwrap() > 0);
    assert!(m["m"]["playhead_ms"].as_i64().unwrap() < m["m"]["server_ms"].as_i64().unwrap());

    // The rules: a link from a member, a repeat, too fast.
    let ack = a
        .post("a2", "chat", "cheap tickets at bonnie-gifts.shop")
        .await;
    assert_eq!(ack["code"], "links");
    tokio::time::sleep(Duration::from_millis(700)).await;
    let ack = a.post("a3", "chat", "drove the N22 for nine years!").await;
    assert_eq!(ack["code"], "repeat");

    // A reply names whom it answers.
    tokio::time::sleep(Duration::from_millis(700)).await;
    j.send(json!({"t": "post", "nonce": "j1", "kind": "chat", "body": "two an hour till three", "reply_to": id}))
        .await;
    let ack = j.until(|f| f["t"] == "ack" && f["nonce"] == "j1").await;
    assert_eq!(ack["state"], "visible");
    let m = reader
        .until(|f| f["t"] == "msg" && f["m"]["author"]["name"] == "Jessie Brown")
        .await;
    assert_eq!(m["m"]["reply_to"]["name"], "Alastair Buchanan");

    // Jessie reports Alastair's message; the screener's queue gets one item.
    let (code, _) = s
        .post("/api/v1/chat/reports", Some(&jessie), json!({"message_id": id, "reason": "abuse", "detail": "Hateful towards the caller on air."}))
        .await;
    assert_eq!(code, 201);
    let item = mods.until(|f| f["t"] == "mod_item").await;
    assert_eq!(item["item"]["kind"], "report");
    assert_eq!(item["item"]["count"], 1);
    assert_eq!(item["item"]["message"]["id"], id);
    let report = item["item"]["id"].as_str().unwrap().to_owned();
    // Reporting yourself is refused.
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&alastair),
            json!({"message_id": id, "reason": "spam"}),
        )
        .await;
    assert_eq!(code, 422);

    // Hide it and time him out: the room loses it, he gets a statement and his standing.
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(ADMIN),
            json!({"op": "hide", "id": id, "reason": "abuse", "then": "timeout", "report": report, "note": "aimed at the caller"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let h = reader.until(|f| f["t"] == "hide").await;
    assert_eq!(h["ids"], json!([id]));
    let st = a.until(|f| f["t"] == "statement").await;
    let text = st["s"]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Kettle FM hid your message “Drove the N22 for nine years”"),
        "{text}"
    );
    assert!(text.contains("stopped you posting for 10 minutes"));
    assert!(text.contains("A moderator decided."));
    assert!(text.contains("within 14 days"));
    let you = a.until(|f| f["t"] == "you").await;
    assert!(you["me"]["timed_out_until_ms"].as_i64().unwrap() > now());
    let done = mods.until(|f| f["t"] == "mod_item_done").await;
    assert_eq!(done["id"], report.as_str());
    let ack = a.post("a4", "chat", "that was unfair").await;
    assert_eq!(ack["code"], "timed_out");

    // The log has both acts, with who and why.
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(ADMIN)).await;
    let actions: Vec<&str> = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(
        actions.contains(&"hide") && actions.contains(&"timeout"),
        "{actions:?}"
    );

    // He asks for a review: it lands in the queue.
    let (code, _) = s
        .post(
            &format!(
                "/api/v1/chat/statements/{}/review",
                st["s"]["id"].as_str().unwrap()
            ),
            Some(&alastair),
            json!({"text": "I was quoting him."}),
        )
        .await;
    assert_eq!(code, 201);
    let review = mods
        .until(|f| f["t"] == "mod_item" && f["item"]["kind"] == "review")
        .await;
    assert_eq!(review["item"]["detail"], "I was quoting him.");

    // A ban takes his past messages out of the room.
    let (code, _) = s
        .moderate(json!({"op": "lift", "account": alastair_id}))
        .await;
    assert_eq!(code, 200);
    sqlx::query("UPDATE chat_message SET state = 'visible' WHERE station_id = $1")
        .bind(s.station)
        .execute(&s.db)
        .await
        .unwrap();
    let (code, body) = s
        .moderate(json!({"op": "ban", "account": alastair_id, "reason": "abuse"}))
        .await;
    assert_eq!(code, 200, "{body}");
    let h = reader.until(|f| f["t"] == "hide").await;
    assert!(h["ids"].as_array().unwrap().contains(&json!(id)));
    let you = a
        .until(|f| f["t"] == "you" && f["me"]["banned"] == true)
        .await;
    assert_eq!(you["me"]["banned"], true);
    let ack = a.post("a5", "chat", "hello?").await;
    assert_eq!(ack["code"], "banned");
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn the_word_filter_holds_for_a_moderator_and_release_shows_it() {
    let s = server().await;
    let (fiona, _) = s
        .chatter("fiona.ferguson@mail.example", "ferry_fiona")
        .await;
    let elspeth = s.staff("Elspeth Gordon").await;
    let (mut reader, _) = Client::open(&s, json!({"t": "hello"})).await;
    let (mut f, _) = Client::open(&s, json!({"t": "hello", "session": fiona})).await;
    let (mut mods, _) = Client::open(&s, json!({"t": "hello", "session": elspeth})).await;

    let ack = f
        .post("f1", "chat", "the councillor is a fucking idiot")
        .await;
    assert_eq!(ack["state"], "held", "{ack}");
    let id = ack["id"].as_i64().unwrap();
    let held = mods.until(|x| x["t"] == "mod_held").await;
    assert_eq!(held["h"]["m"]["id"], id);
    assert_eq!(held["h"]["flagged"], "rustrict");
    assert_eq!(
        held["h"]["release_at_ms"],
        Value::Null,
        "the filter waits for a moderator"
    );
    assert!(
        !reader.gets(|x| x["t"] == "msg", 600).await,
        "nobody else sees a held message"
    );

    let (code, body) = s.moderate(json!({"op": "release", "id": id})).await;
    assert_eq!(code, 200, "{body}");
    let m = reader.until(|x| x["t"] == "msg").await;
    assert_eq!(m["m"]["id"], id);
    assert!(f.gets(|x| x["t"] == "released", 2000).await);

    // A second flagged message, rejected: the statement names the automated step.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let ack = f.post("f2", "chat", "you are all fucking morons").await;
    let id2 = ack["id"].as_i64().unwrap();
    let (code, _) = s
        .moderate(json!({"op": "hide", "id": id2, "reason": "abuse"}))
        .await;
    assert_eq!(code, 200);
    let st = f.until(|x| x["t"] == "statement").await;
    let text = st["s"]["text"].as_str().unwrap();
    assert!(text.contains("did not show your message"), "{text}");
    assert!(text.contains("An automatic filter flagged it and a moderator decided."));
    assert_eq!(st["s"]["automated"], true);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn the_studio_lane_is_private_and_answers_come_back() {
    let s = server().await;
    let (lesley, _) = s.chatter("lesley.kerr@mail.example", "lesley.k").await;
    let elspeth = s.staff("Elspeth Gordon").await;
    let (mut reader, _) = Client::open(&s, json!({"t": "hello"})).await;
    let (mut l, _) = Client::open(&s, json!({"t": "hello", "session": lesley})).await;
    let (mut mods, _) = Client::open(&s, json!({"t": "hello", "session": elspeth})).await;

    let ack = l
        .post(
            "l1",
            "studio",
            "Rory, my dad drove the N22 in the nineties.",
        )
        .await;
    assert_eq!(ack["state"], "visible");
    let id = ack["id"].as_i64().unwrap();
    let lane = mods.until(|x| x["t"] == "mod_studio").await;
    assert_eq!(lane["s"]["m"]["id"], id);
    assert!(l.gets(|x| x["t"] == "mine", 2000).await);
    assert!(
        !reader.gets(|x| x["t"] == "msg", 500).await,
        "the room never sees the lane"
    );

    let (code, _) = s
        .moderate(json!({"op": "reply", "id": id, "body": "Rory will read it after the news."}))
        .await;
    assert_eq!(code, 200);
    let p = l.until(|x| x["t"] == "private").await;
    assert_eq!(p["body"], "Rory will read it after the news.");
    let (code, _) = s.moderate(json!({"op": "invite", "id": id})).await;
    assert_eq!(code, 200);
    let p = l
        .until(|x| x["t"] == "private" && x["invite"] == true)
        .await;
    assert_eq!(p["invite"], true);
    let lane = mods
        .until(|x| x["t"] == "mod_studio" && x["s"]["invited_ms"].is_i64())
        .await;
    assert_eq!(lane["s"]["reply"], "Rory will read it after the news.");

    // A request is a room message, marked.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let ack = l
        .post("l2", "request", "Tide's Out, for the night-shift lot")
        .await;
    assert_eq!(ack["state"], "visible");
    let m = reader.until(|x| x["t"] == "msg").await;
    assert_eq!(m["m"]["kind"], "request");
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_reconnect_recovers_and_erasure_starts_a_new_epoch() {
    let s = server().await;
    let (niall, niall_id) = s
        .chatter("niall.anderson@mail.example", "Niall Anderson")
        .await;
    let (mut reader, w) = Client::open(&s, json!({"t": "hello"})).await;
    let epoch = w["epoch"].as_str().unwrap().to_owned();
    let (mut n, _) = Client::open(&s, json!({"t": "hello", "session": niall})).await;
    let first = n.post("n1", "chat", "first").await["id"].as_i64().unwrap();
    let m = reader.until(|x| x["t"] == "msg").await;
    let offset = m["offset"].as_u64().unwrap();
    drop(reader);
    for (i, body) in ["second", "third", "fourth"].iter().enumerate() {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let ack = n.post(&format!("n{}", i + 2), "chat", body).await;
        assert_eq!(ack["state"], "visible");
    }
    let (_, w) = Client::open(
        &s,
        json!({"t": "hello", "since": {"epoch": epoch, "offset": offset}}),
    )
    .await;
    assert_eq!(w["recovered"], true);
    let missed = w["missed"].as_array().unwrap();
    assert_eq!(missed.len(), 3, "{missed:?}");
    assert_eq!(missed[0]["m"]["body"], "second");

    // Another epoch, or too old: a fresh snapshot instead.
    let (_, w) = Client::open(
        &s,
        json!({"t": "hello", "since": {"epoch": "gone", "offset": 1}}),
    )
    .await;
    assert_eq!(w["recovered"], false);
    assert_eq!(w["history"].as_array().unwrap().len(), 4);

    // Erasure: his messages go from the room and the database, and the epoch changes.
    let (code, body) = s
        .post(
            &format!("/api/v1/chat/admin/accounts/{niall_id}/erase"),
            Some(ADMIN),
            json!({}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["messages"], 4);
    let (_, w2) = Client::open(
        &s,
        json!({"t": "hello", "since": {"epoch": epoch, "offset": offset}}),
    )
    .await;
    assert_eq!(w2["recovered"], false);
    assert_ne!(w2["epoch"], epoch.as_str());
    assert!(
        w2["history"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["id"] != first)
    );
    let (n_left,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM chat_message WHERE station_id = $1")
            .bind(s.station)
            .fetch_one(&s.db)
            .await
            .unwrap();
    assert_eq!(n_left, 0);
    // His session ended with the erasure.
    let (code, _) = s.get("/api/v1/chat/me", Some(&niall)).await;
    assert_eq!(code, 401);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn notices_escalation_and_the_retention_sweep() {
    let s = server().await;
    let elspeth = s.staff("Elspeth Gordon").await;
    let (mut mods, _) = Client::open(&s, json!({"t": "hello", "session": elspeth})).await;

    // A notice from someone without an account needs the Article 16 fields.
    let (code, _) = s
        .post(
            "/api/v1/chat/notices",
            None,
            json!({"reason": "illegal", "location": "", "detail": "x", "email": "k@mail.example"}),
        )
        .await;
    assert_eq!(code, 422);
    let (code, body) = s
        .post(
            "/api/v1/chat/notices",
            None,
            json!({"reason": "illegal", "location": "Saturday's replay at 1:12:40", "detail": "It gives my street and house number.", "name": "K Ferguson", "email": "k.ferguson@mail.example", "good_faith": true}),
        )
        .await;
    assert_eq!(code, 201, "{body}");
    let item = mods.until(|x| x["t"] == "mod_item").await;
    assert_eq!(item["item"]["kind"], "notice");
    assert_eq!(item["item"]["contact"]["email"], "k.ferguson@mail.example");
    let report = item["item"]["id"].as_str().unwrap().to_owned();

    // An intimate-image report carries its 48-hour clock.
    let (iona, _) = s
        .chatter("iona.macintosh@mail.example", "Iona MacIntosh")
        .await;
    let (tavish, tavish_id) = s.chatter("tavish@mail.example", "Tavish").await;
    let (mut t, _) = Client::open(&s, json!({"t": "hello", "session": tavish})).await;
    let id = t.post("t1", "chat", "look at this one").await["id"]
        .as_i64()
        .unwrap();
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&iona),
            json!({"message_id": id, "reason": "intimate_image"}),
        )
        .await;
    assert_eq!(code, 201);
    let item = mods
        .until(|x| x["t"] == "mod_item" && x["item"]["reason"] == "intimate_image")
        .await;
    let due = item["item"]["due_ms"].as_i64().unwrap();
    assert!((due - now() - 48 * 3_600_000).abs() < 120_000);

    // Escalate the notice: an evidence hold.
    let (code, body) = s
        .moderate(json!({"op": "escalate", "report": report}))
        .await;
    assert_eq!(code, 200, "{body}");
    let (_, holds) = s.get("/api/v1/chat/admin/holds", Some(ADMIN)).await;
    assert!(
        holds["holds"].as_array().unwrap().len() >= 2,
        "the intimate-image report preserved at once, and the escalation"
    );

    // The sweep: an old message goes unless a hold keeps it.
    sqlx::query(
        "UPDATE chat_message SET server_at = now() - interval '7 hours' WHERE station_id = $1",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    let swept = weavesignal_chat::store::sweep(
        &s.db,
        s.station.into(),
        Some(6 * 3600),
        false,
        Some(14 * 86_400),
        None,
    )
    .await
    .unwrap();
    assert_eq!(swept.messages, 0, "the reported image is under a hold");
    sqlx::query("UPDATE chat_hold SET released_at = now() WHERE station_id = $1")
        .bind(s.station)
        .execute(&s.db)
        .await
        .unwrap();
    // The station's own retention hold (ADR-0116), by a window of broadcast time, keeps it too.
    sqlx::query(
        "INSERT INTO retention_hold (station_id, reason, placed_by, window_from, window_to)
         VALUES ($1, 'complaint about the show', 'Elspeth Gordon', now() - interval '1 day', now())",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    let swept = weavesignal_chat::store::sweep(
        &s.db,
        s.station.into(),
        Some(6 * 3600),
        false,
        Some(14 * 86_400),
        None,
    )
    .await
    .unwrap();
    assert_eq!(swept.messages, 0, "a retention hold's window covers it");
    sqlx::query(
        "UPDATE retention_hold SET released_at = now(), released_by = 'Elspeth Gordon', release_reason = 'answered'
         WHERE station_id = $1",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    let swept = weavesignal_chat::store::sweep(
        &s.db,
        s.station.into(),
        Some(6 * 3600),
        false,
        Some(14 * 86_400),
        None,
    )
    .await
    .unwrap();
    assert_eq!(swept.messages, 1);

    // Export holds what is kept about a chatter, never the email address.
    let (code, body) = s
        .get(
            &format!("/api/v1/chat/admin/accounts/{tavish_id}/export"),
            Some(ADMIN),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(body["account"]["name"], "Tavish");
    assert!(!body.to_string().contains("tavish@mail.example"));

    // The station's settings and the risk-assessment template.
    let (code, _) = s.get("/api/v1/chat/admin/settings", None).await;
    assert_eq!(code, 401);
    let (code, info) = s.get("/api/v1/chat/info", None).await;
    assert_eq!(code, 200);
    assert_eq!(info["retention"]["addresses"], "IP addresses are not kept.");
    assert_eq!(info["reasons"].as_array().unwrap().len(), 8);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_swept_message_id_is_never_handed_out_again() {
    // Review 0025, S2: the chat hands ids out from one more than the highest it can find at
    // start-up; with the newest messages swept, the floor must still remember them.
    let s = server().await;
    let (morag, _) = s.chatter("morag@mail.example", "Morag").await;
    let (mut m, _) = Client::open(&s, json!({"t": "hello", "session": morag})).await;
    let ack = m.post("f1", "chat", "the newest message").await;
    let last = ack["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("not posted: {ack}"));

    sqlx::query(
        "UPDATE chat_message SET server_at = now() - interval '7 hours' WHERE station_id = $1",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    let swept =
        weavesignal_chat::store::sweep(&s.db, s.station.into(), Some(6 * 3600), false, None, None)
            .await
            .unwrap();
    assert_eq!(swept.messages, 1);

    let next = weavesignal_chat::store::next_message_id(&s.db, s.station.into())
        .await
        .unwrap();
    assert!(
        next > last,
        "a restart would hand out {next} again, already given to a swept message ({last})"
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn under_no_retention_messages_go_when_the_scheduled_show_ends() {
    // ADR-0149: a message written during a scheduled episode goes when the episode ends, not
    // six hours after it was written; with nothing scheduled the six hours still apply.
    let s = server().await;
    let mut ids = Vec::new();
    for n in 0..5 {
        let (session, _) = s
            .chatter(&format!("chatter{n}@mail.example"), &format!("Chatter{n}"))
            .await;
        let (mut c, _) = Client::open(&s, json!({"t": "hello", "session": session})).await;
        let ack = c.post("e", "chat", &format!("line {n}")).await;
        ids.push(
            ack["id"]
                .as_i64()
                .unwrap_or_else(|| panic!("not posted: {ack}")),
        );
    }
    let [before_show, in_long, in_short, quiet_old, quiet_new] = ids[..] else {
        unreachable!()
    };
    let aged = |id: i64, hours: f64| {
        let db = s.db.clone();
        let station = s.station;
        async move {
            // This station's message only: ids are per station, and another test's room has
            // the same numbers.
            sqlx::query(
                "UPDATE chat_message SET server_at = now() - make_interval(secs => $2)
                 WHERE id = $1 AND station_id = $3",
            )
            .bind(id)
            .bind(hours * 3600.0)
            .bind(station)
            .execute(&db)
            .await
            .unwrap();
        }
    };
    let left = || {
        let db = s.db.clone();
        let station = s.station;
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM chat_message WHERE station_id = $1 ORDER BY id",
            )
            .bind(station)
            .fetch_all(&db)
            .await
            .unwrap()
        }
    };
    let sweep = || async {
        weavesignal_chat::store::sweep(&s.db, s.station.into(), Some(6 * 3600), true, None, None)
            .await
            .unwrap()
    };

    // A long episode: on air since 10 hours ago, 12 hours long. Its early message is 9 hours
    // old and stays; a message from before it began is 12 hours old and goes.
    let show = uuid::Uuid::new_v4();
    let series = uuid::Uuid::new_v4();
    let long = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO show (id, station_id, slug, name, colour_slot, slot_weekday, slot_start, slot_minutes)
         VALUES ($1, $2, 'the-long-night', 'The Long Night', 1, 1, 0, 120)",
    )
    .bind(show)
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO series (id, station_id, show_id, number) VALUES ($1, $2, $3, 1)")
        .bind(series)
        .bind(s.station)
        .bind(show)
        .execute(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at, minutes)
         VALUES ($1, $2, $3, 1, now() - interval '10 hours', 720)",
    )
    .bind(long)
    .bind(s.station)
    .bind(series)
    .execute(&s.db)
    .await
    .unwrap();
    aged(before_show, 12.0).await;
    aged(in_long, 9.0).await;
    aged(in_short, 0.1).await;
    aged(quiet_old, 0.2).await;
    aged(quiet_new, 0.05).await;
    let swept = sweep().await;
    assert_eq!(swept.messages, 1, "only the message before the show goes");
    assert_eq!(left().await, vec![in_long, in_short, quiet_old, quiet_new]);

    // The long episode is long past; a two-hour one, its length from the show's slot, ended
    // 3 hours ago. A message from 4 hours ago, written in it, goes though it is under six
    // hours old; one from 1 hour ago, after it, stays; the 9-hour-old one is unscheduled now.
    sqlx::query("UPDATE episode SET air_at = now() - interval '30 hours' WHERE id = $1")
        .bind(long)
        .execute(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at)
         VALUES ($1, $2, $3, 2, now() - interval '5 hours')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(s.station)
    .bind(series)
    .execute(&s.db)
    .await
    .unwrap();
    aged(in_short, 4.0).await;
    aged(quiet_old, 7.0).await;
    aged(quiet_new, 1.0).await;
    let swept = sweep().await;
    assert_eq!(
        swept.messages, 3,
        "the 9-hour-old, the 7-hour-old and the one written in the ended show"
    );
    assert_eq!(left().await, vec![quiet_new]);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn one_address_sends_ten_notices_in_ten_minutes_and_no_more() {
    // ADR-0120 (review 0025, C1): moderators' queue cannot be flooded from one host. Every
    // attempt counts, a refused one included; the eleventh is refused before it is read.
    let s = server().await;
    let notice = json!({"reason": "illegal", "location": "Saturday's replay at 1:12:40", "detail": "It gives my street and house number.", "name": "K Ferguson", "email": "k.ferguson@mail.example", "good_faith": true});
    for n in 1..=10 {
        let (code, body) = s.post("/api/v1/chat/notices", None, notice.clone()).await;
        assert_eq!(code, 201, "notice {n}: {body}");
    }
    let (code, body) = s.post("/api/v1/chat/notices", None, notice).await;
    assert_eq!(code, 429, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("Too many notices"),
        "{body}"
    );
}

/// A staff session as the control plane makes it (ADR-0127): an account and a token.
async fn staff_signed_in(
    s: &Server,
    username: &str,
    name: &str,
    role: weavesignal_model::staff::Role,
) -> (uuid::Uuid, String) {
    use weavesignal_model::staff;
    let station = weavesignal_model::StationId::from(s.station);
    let hash = staff::hash_password("a long enough phrase").unwrap();
    let a = staff::create_account(&s.db, station, username, name, role, &hash)
        .await
        .unwrap();
    let token = staff::new_token();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    staff::create_session(
        &s.db,
        station,
        a.id,
        &staff::token_hash(&token),
        now + 3_600_000,
    )
    .await
    .unwrap();
    (a.id, token)
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn staff_moderate_as_themselves_and_lose_the_chat_when_turned_off() {
    use weavesignal_model::staff::{self, Role};
    let s = server().await;
    let (_, alastair_id) = s
        .chatter("alastair.buchanan@mail.example", "Alastair Buchanan")
        .await;
    let (elspeth_id, elspeth) =
        staff_signed_in(&s, "elspeth", "Elspeth Gordon", Role::Screener).await;
    let (_, morag) = staff_signed_in(&s, "morag", "Morag Campbell", Role::Admin).await;

    // Elspeth's chat session carries her account's name, whatever she types.
    let (code, body) = s
        .post(
            "/api/v1/chat/staff/session",
            Some(&elspeth),
            json!({"name": "Someone Else"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let chat_session = body["session"].as_str().unwrap().to_owned();
    let (_, me) = s.get("/api/v1/chat/me", Some(&chat_session)).await;
    assert_eq!(me["me"]["name"], "Elspeth Gordon", "{me}");

    // She moderates with her staff session; the log names her, not "The studio".
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&elspeth),
            json!({"op": "timeout", "account": alastair_id, "minutes": 5, "reason": "spam"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(&morag)).await;
    let actors: Vec<&str> = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["actor"].as_str().unwrap())
        .collect();
    assert_eq!(actors, vec!["Elspeth Gordon"], "{log}");

    // A screener is not an admin: the chat's settings and exports are refused.
    let (code, _) = s.get("/api/v1/chat/admin/holds", Some(&elspeth)).await;
    assert_eq!(code, 403);
    let (code, _) = s.get("/api/v1/chat/admin/holds", Some(&morag)).await;
    assert_eq!(code, 200);

    // Turned off: her next action is refused, and her chat session has ended (review 0025, C2).
    staff::change_account(
        &s.db,
        weavesignal_model::StationId::from(s.station),
        elspeth_id,
        &staff::Change {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (code, _) = s
        .post(
            "/api/v1/chat/mod",
            Some(&elspeth),
            json!({"op": "lift", "account": alastair_id}),
        )
        .await;
    assert_eq!(code, 401);
    let (code, _) = s.get("/api/v1/chat/me", Some(&chat_session)).await;
    assert_eq!(code, 401);
}

/// What an address was kept as before ADR-0125: the plain SHA-256 of its lower-case form.
fn plain_sha256(email: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(email.trim().to_lowercase().as_bytes()).to_vec()
}

async fn email_row(s: &Server, id: &str) -> (Option<Vec<u8>>, i16) {
    sqlx::query_as("SELECT email_hash, email_hash_version FROM chat_account WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&s.db)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_address_and_its_code_are_kept_as_keyed_hashes_and_the_chatter_signs_in_again() {
    let s = server().await;
    let key = EmailKey::parse(KEY).unwrap();
    let email = "morag.henderson@mail.example";
    let (_, id) = s.chatter(email, "Morag Henderson").await;

    let (hash, version) = email_row(&s, &id).await;
    let hash = hash.expect("an email account has a hash");
    assert_eq!(version, 2);
    assert_eq!(hash, key.email_hash(email));
    assert_ne!(hash, plain_sha256(email), "not the plain SHA-256");
    let other = EmailKey::parse("another-station-key-0123456789abcdef").unwrap();
    assert_ne!(hash, other.email_hash(email), "another key, another hash");

    // The code row is keyed too, and carries the keyed address hash.
    let code = s.code_for(email);
    let (e, c): (Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT email_hash, code_hash FROM chat_signin WHERE station_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(s.station)
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(e, key.email_hash(email));
    assert_eq!(c, key.code_hash(email, &code));
    assert_ne!(e, plain_sha256(email));

    // The same person, another spelling of the case: the same account.
    let (status, _) = s
        .post(
            "/api/v1/chat/signin/start",
            None,
            json!({"email": "morag.henderson@MAIL.example"}),
        )
        .await;
    assert_eq!(status, 202);
    let again = s.code_for("morag.henderson@mail.example");
    let (status, body) = s
        .post(
            "/api/v1/chat/signin/finish",
            None,
            json!({"email": "morag.henderson@mail.example", "code": again}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["me"]["id"], id.as_str());
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_account_with_a_plain_hash_is_re_keyed_only_by_a_correct_code() {
    let s = server().await;
    let key = EmailKey::parse(KEY).unwrap();
    let email = "rory.maclean@mail.example";
    let (_, id) = s.chatter(email, "Rory MacLean").await;
    // Turn the row back into what a station held before ADR-0125.
    sqlx::query(
        "UPDATE chat_account SET email_hash = $2, email_hash_version = 1 WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(plain_sha256(email))
    .execute(&s.db)
    .await
    .unwrap();
    sqlx::query("DELETE FROM chat_signin WHERE station_id = $1")
        .bind(s.station)
        .execute(&s.db)
        .await
        .unwrap();
    assert_eq!(
        weavesignal_chat::store::legacy_email_hashes(&s.db, s.station.into())
            .await
            .unwrap(),
        1
    );

    let (status, _) = s
        .post("/api/v1/chat/signin/start", None, json!({"email": email}))
        .await;
    assert_eq!(status, 202);
    let code = s.code_for(email);

    // Knowing the address is not enough: a wrong code re-keys nothing and signs nobody in.
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let (status, _) = s
        .post(
            "/api/v1/chat/signin/finish",
            None,
            json!({"email": email, "code": wrong}),
        )
        .await;
    assert_eq!(status, 422);
    let (hash, version) = email_row(&s, &id).await;
    assert_eq!((hash, version), (Some(plain_sha256(email)), 1));

    // The right code signs the same account in and re-keys it, once.
    let (status, body) = s
        .post(
            "/api/v1/chat/signin/finish",
            None,
            json!({"email": email, "code": code}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["me"]["id"],
        id.as_str(),
        "the same account, not a new one"
    );
    let (hash, version) = email_row(&s, &id).await;
    assert_eq!((hash, version), (Some(key.email_hash(email)), 2));
    assert_eq!(
        weavesignal_chat::store::legacy_email_hashes(&s.db, s.station.into())
            .await
            .unwrap(),
        0
    );

    // And the next sign-in finds it by the keyed hash.
    let (status, _) = s
        .post("/api/v1/chat/signin/start", None, json!({"email": email}))
        .await;
    assert_eq!(status, 202);
    let code = s.code_for(email);
    let (status, body) = s
        .post(
            "/api/v1/chat/signin/finish",
            None,
            json!({"email": email, "code": code}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["me"]["id"], id.as_str());
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM chat_account WHERE station_id = $1 AND kind = 'email'",
    )
    .bind(s.station)
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn the_service_will_not_start_without_the_key() {
    let station = uuid::Uuid::new_v4();
    let mut c = Config::from_toml(
        &SAMPLE.replace("00000000-0000-0000-0000-000000000000", &station.to_string()),
        "test",
    )
    .unwrap();
    // The key is checked before the database is, so no database is needed to see it refused.
    c.database_url = Some("postgres://nobody@127.0.0.1:1/none".into());
    assert!(c.email_key.is_none());
    let err = weavesignal_chat::start(c).await.err().expect("refused");
    let text = format!("{err:#}");
    assert!(text.contains("WEAVESIGNAL_CHAT_EMAIL_KEY"), "{text}");
    assert!(text.contains("32"), "{text}");
}

/// The binary itself: a missing key and a short one both stop it, naming the variable.
#[test]
fn the_binary_exits_with_a_clear_error_for_a_missing_or_short_key() {
    let dir = std::env::temp_dir().join(format!("weavesignal-chat-key-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let conf = dir.join("chat.toml");
    std::fs::write(&conf, SAMPLE).unwrap();
    let run = |key: Option<&str>| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_weavesignal-chat"));
        cmd.arg("--config")
            .arg(&conf)
            .arg("serve")
            .env_remove("WEAVESIGNAL_CHAT_EMAIL_KEY")
            .env(
                "WEAVESIGNAL_DATABASE_URL",
                "postgres://nobody@127.0.0.1:1/none",
            );
        if let Some(k) = key {
            cmd.env("WEAVESIGNAL_CHAT_EMAIL_KEY", k);
        }
        cmd.output().unwrap()
    };
    for key in [None, Some("too-short")] {
        let out = run(key);
        assert_eq!(out.status.code(), Some(2), "{key:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("WEAVESIGNAL_CHAT_EMAIL_KEY"), "{key:?}: {err}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Two first sign-ins at once from one address whose account still holds the plain hash: both
/// must find that account, not one of them be offered a new one (which the address's unique
/// index would then refuse as "That name is taken").
#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn concurrent_first_sign_ins_from_a_legacy_address_find_the_same_account() {
    let s = server().await;
    let key = EmailKey::parse(KEY).unwrap();
    let st: weavesignal_model::StationId = s.station.into();
    // Several addresses and several sign-ins each, so the moment between the keyed look-up and
    // the re-key is hit at least once.
    for n in 0..12 {
        let email = format!("eilidh.campbell{n}@mail.example");
        // Made directly, as a station held it before ADR-0125 (each sign-in by the routes
        // would spend this address's code allowance).
        let made = weavesignal_chat::store::create_account(
            &s.db,
            st,
            &weavesignal_chat::store::NewAccount {
                kind: weavesignal_chat::rules::Kind::Email,
                email_hash: Some(plain_sha256(&email)),
                name: &format!("Eilidh Campbell {n}"),
                standing: weavesignal_chat::rules::Standing::Member,
                terms_version: None,
                age_declared: true,
            },
        )
        .await
        .unwrap();
        let id = made.id.to_string();
        sqlx::query("UPDATE chat_account SET email_hash_version = 1 WHERE id = $1::uuid")
            .bind(&id)
            .execute(&s.db)
            .await
            .unwrap();
        let tries = (0..8).map(|_| {
            let (db, key, email) = (s.db.clone(), key.clone(), email.clone());
            tokio::spawn(async move {
                weavesignal_chat::store::account_by_verified_email(&db, st, &key, &email).await
            })
        });
        for t in futures_util::future::join_all(tries).await {
            let a = t
                .unwrap()
                .unwrap()
                .expect("the existing account, every time");
            assert_eq!(a.id.to_string(), id, "{email}");
        }
        assert_eq!(email_row(&s, &id).await, (Some(key.email_hash(&email)), 2));
    }
}

/// Erasure (ADR-0122) clears the hash whichever form it is in, and the address then finds
/// nothing: a later sign-in from it is a new person.
#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_erased_account_is_found_by_neither_hash() {
    let s = server().await;
    let key = EmailKey::parse(KEY).unwrap();
    let st: weavesignal_model::StationId = s.station.into();
    let keyed = "iain.fraser@mail.example";
    let legacy = "isla.robertson@mail.example";
    let (_, keyed_id) = s.chatter(keyed, "Iain Fraser").await;
    let (_, legacy_id) = s.chatter(legacy, "Isla Robertson").await;
    sqlx::query(
        "UPDATE chat_account SET email_hash = $2, email_hash_version = 1 WHERE id = $1::uuid",
    )
    .bind(&legacy_id)
    .bind(plain_sha256(legacy))
    .execute(&s.db)
    .await
    .unwrap();
    for id in [&keyed_id, &legacy_id] {
        let (status, body) = s
            .post(
                &format!("/api/v1/chat/admin/accounts/{id}/erase"),
                Some(ADMIN),
                json!({}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(email_row(&s, id).await.0, None, "the hash is gone");
    }
    for email in [keyed, legacy] {
        assert!(
            weavesignal_chat::store::account_by_verified_email(&s.db, st, &key, email)
                .await
                .unwrap()
                .is_none(),
            "{email}"
        );
    }
    assert_eq!(
        weavesignal_chat::store::legacy_email_hashes(&s.db, st)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn the_minimum_age_follows_the_jurisdiction_until_the_station_sets_its_own() {
    // ADR-0148: the age a chatter declares follows the station's retention preset; a station
    // can set its own; a stored 13 (the only default before) follows the preset.
    let s = server().await;
    let station = weavesignal_model::StationId::from(s.station);
    let settings = |body: Value| {
        http(
            "PUT",
            format!("{}/api/v1/chat/admin/settings", s.base),
            Some(ADMIN.into()),
            Some(body),
        )
    };
    let age = || async { s.get("/api/v1/chat/info", None).await.1["min_age"].clone() };
    assert_eq!(age().await, 13, "No retention: 13");
    let (code, saved) = settings(json!({"guests": true})).await;
    assert_eq!(code, 200, "{saved}");
    assert_eq!(saved["min_age"], Value::Null, "following the jurisdiction");

    // Germany: the next sign-up asks for 16 at once, without the chat's ten-minute reload.
    let de = weavesignal_model::retention::Setting {
        preset: "de".into(),
        ..Default::default()
    };
    weavesignal_model::retention::save_setting(&s.db, station, &de, "test")
        .await
        .unwrap();
    let (code, p) = s
        .post("/api/v1/chat/guest", None, json!({"name": "Catriona"}))
        .await;
    assert_eq!(code, 422, "{p}");
    assert!(
        p.to_string().contains("Confirm you are 16 or older."),
        "{p}"
    );
    assert_eq!(age().await, 16);

    // The station's own age wins; a stored 13 follows the jurisdiction again.
    let (code, _) = settings(json!({"guests": true, "min_age": 18})).await;
    assert_eq!(code, 200);
    assert_eq!(age().await, 18);
    let (code, saved) = settings(json!({"guests": true, "min_age": 13})).await;
    assert_eq!(code, 200);
    assert_eq!(saved["min_age"], Value::Null);
    assert_eq!(age().await, 16);
}

/// A control plane for `station` as `serve` assembles the parts this needs (the studio's
/// routes under the API gate), on a port of its own; its address.
async fn control_plane(pool: &sqlx::PgPool, station: uuid::Uuid) -> String {
    use std::sync::Arc;
    use weavesignal_control::api::{Api, ApiConfig, layer};
    use weavesignal_control::apikey::ApiKey;
    use weavesignal_control::app::{AppState, router};
    use weavesignal_control::carts::CartsLink;
    use weavesignal_control::config::{CartsConfig, JoinConfig};
    use weavesignal_control::desk::DeskClient;
    use weavesignal_control::hub::Hub;
    use weavesignal_control::staff::Staff;
    use weavesignal_control::studio::Studio;
    use weavesignal_model::{Station, StationId, ensure_station};

    let station = ensure_station(
        pool,
        &Station::new(StationId::from(station), "Modes FM").unwrap(),
    )
    .await
    .unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        Some(ApiKey::parse(TOKEN_KEY).unwrap()),
        ApiConfig {
            token_per_minute: TOKEN_PER_MINUTE,
            ..ApiConfig::default()
        },
        false,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    staff.restore(station.id).await;
    staff.api.restore().await;
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));
    let service = layer(app, staff).into_make_service_with_connect_info::<SocketAddr>();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    format!("http://{addr}")
}

const TOKEN_KEY: &str = "chat-test-token-key-0123456789abcdef";
/// Each token's limit on the control plane in these tests, so the 429 is near.
const TOKEN_PER_MINUTE: u32 = 5;

/// A `POST` through the chat that keeps the answer's `Retry-After`.
async fn post_retry(url: String, bearer: String, body: Value) -> (u16, Option<String>) {
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let r = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .send_json(body)
            .unwrap();
        let retry = r
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        (r.status().as_u16(), retry)
    })
    .await
    .unwrap()
}

/// Issue 2: the station's proxy sends `/api/v1/chat/…` to the chat, so an integration's token
/// with `chat:moderate` reaches the room's modes only through it. The chat passes the token on,
/// and the control plane's gate decides as it does for its own routes: the scope, revocation,
/// expiry, an unknown token, the per-minute limit. No other studio route of the chat takes a
/// token.
#[tokio::test]
#[ignore = "needs PostgreSQL: just test-db"]
async fn a_chat_moderate_token_switches_the_modes_through_the_chat_and_nothing_else() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let station = uuid::Uuid::new_v4();
    let control = control_plane(&pool, station).await;
    let s = server_for(station, &control).await;
    let make = |scopes: Value| {
        let control = control.clone();
        async move {
            let (code, made) = http(
                "POST",
                format!("{control}/api/v1/tokens"),
                Some(ADMIN.to_owned()),
                Some(json!({"name": "Stream deck", "scopes": scopes})),
            )
            .await;
            assert_eq!(code, 201, "{made}");
            (
                made["secret"].as_str().unwrap().to_owned(),
                made["token"]["id"].as_str().unwrap().to_owned(),
            )
        }
    };
    let (moderate, moderate_id) = make(json!(["chat:moderate"])).await;
    let (reader, _) = make(json!(["studio:read"])).await;
    let (burst, _) = make(json!(["chat:moderate"])).await;
    let (expiring, expiring_id) = make(json!(["chat:moderate"])).await;
    let slow = |on: bool| json!({"op": "mode", "mode": "slow", "on": on});

    // Through the chat, as the proxy sends it: switched, and the new view comes back.
    let (code, v) = s
        .post("/api/v1/chat/modes", Some(&moderate), slow(true))
        .await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["chat"]["slow"], true, "{v}");
    let (_, view) = http(
        "GET",
        format!("{control}/api/v1/studio"),
        Some(ADMIN.to_owned()),
        None,
    )
    .await;
    assert_eq!(view["chat"]["slow"], true, "the control plane holds it");

    // A token without the scope, and one that is not a token the station made.
    let (code, v) = s
        .post("/api/v1/chat/modes", Some(&reader), slow(false))
        .await;
    assert_eq!(code, 403, "{v}");
    assert!(v.to_string().contains("chat:moderate"), "{v}");
    let made_up = format!("rsa_{}", "0".repeat(64));
    let (code, _) = s
        .post("/api/v1/chat/modes", Some(&made_up), slow(false))
        .await;
    assert_eq!(code, 401);

    // The token opens nothing else of the chat's: the studio's sign-in, moderation and its
    // log, the admin's settings.
    let (code, _) = s
        .post(
            "/api/v1/chat/staff/session",
            Some(&moderate),
            json!({"name": "Deck"}),
        )
        .await;
    assert_eq!(code, 401, "a token is not the studio");
    let (code, _) = s
        .post(
            "/api/v1/chat/mod",
            Some(&moderate),
            json!({"op": "release_all"}),
        )
        .await;
    assert_eq!(code, 401, "nor a moderator");
    let (code, _) = s.get("/api/v1/chat/mod/log", Some(&moderate)).await;
    assert_eq!(code, 401, "nor a reader of the log");
    let (code, _) = s.get("/api/v1/chat/admin/settings", Some(&moderate)).await;
    assert_eq!(code, 401, "nor an admin");

    // The station key and staff still switch the modes as before.
    let (code, v) = s.post("/api/v1/chat/modes", Some(ADMIN), slow(false)).await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["chat"]["slow"], false);

    // Over its limit: 429, and when to try again, as the control plane said it.
    let mut answers = Vec::new();
    for _ in 0..=TOKEN_PER_MINUTE {
        answers.push(
            post_retry(
                format!("{}/api/v1/chat/modes", s.base),
                burst.clone(),
                slow(false),
            )
            .await,
        );
    }
    assert_eq!(answers[0], (200, None));
    let (code, retry) = answers.last().unwrap();
    assert_eq!(*code, 429, "{answers:?}");
    assert!(
        retry
            .as_deref()
            .is_some_and(|r| r.parse::<u64>().is_ok_and(|s| s >= 1)),
        "{answers:?}"
    );

    // Expired: the control plane reads tokens back when it starts, so one whose time ran out
    // meanwhile is refused by the next, through its chat.
    sqlx::query(
        "UPDATE api_token SET expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
    )
    .bind(&expiring_id)
    .execute(&pool)
    .await
    .unwrap();
    let control_again = control_plane(&pool, station).await;
    let s_again = server_for(station, &control_again).await;
    let (code, v) = s_again
        .post("/api/v1/chat/modes", Some(&expiring), slow(true))
        .await;
    assert_eq!(code, 401, "{v}");
    let (code, v) = s_again
        .post("/api/v1/chat/modes", Some(&moderate), slow(true))
        .await;
    assert_eq!(code, 200, "a live token still works after the restart: {v}");

    // Revoked: refused at once, through the chat too.
    let (code, _) = http(
        "DELETE",
        format!("{control}/api/v1/tokens/{moderate_id}"),
        Some(ADMIN.to_owned()),
        None,
    )
    .await;
    assert_eq!(code, 204);
    let (code, v) = s
        .post("/api/v1/chat/modes", Some(&moderate), slow(true))
        .await;
    assert_eq!(code, 401, "{v}");
}

/// A show with one episode `starts` seconds from now (negative: started already), `minutes`
/// long; returns nothing, the chat reads it from the schedule.
async fn scheduled(s: &Server, number: i32, starts: i64, minutes: i32) {
    let show = uuid::Uuid::new_v4();
    let series = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO show (id, station_id, slug, name, colour_slot) VALUES ($1, $2, $3, 'The Kettle Phone-In', 1)",
    )
    .bind(show)
    .bind(s.station)
    .bind(format!("phone-in-{number}"))
    .execute(&s.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO series (id, station_id, show_id, number) VALUES ($1, $2, $3, 1)")
        .bind(series)
        .bind(s.station)
        .bind(show)
        .execute(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at, minutes)
         VALUES ($1, $2, $3, 1, date_trunc('second', now()) + make_interval(secs => $4), $5)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(s.station)
    .bind(series)
    .bind(starts as f64)
    .bind(minutes)
    .execute(&s.db)
    .await
    .unwrap();
}

// ---- avatars (ADR-0235) ----

/// A PNG of one colour, with a text chunk as a camera or an editor leaves one.
fn picture(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.add_text_chunk("Location".into(), "56.4907 N, 4.2026 W".into())
        .unwrap();
    let mut w = e.write_header().unwrap();
    let data: Vec<u8> = rgb
        .iter()
        .copied()
        .cycle()
        .take((width * height * 3) as usize)
        .collect();
    w.write_image_data(&data).unwrap();
    w.finish().unwrap();
    out
}

/// A request with a raw body and a raw answer: status, content type, ETag, bytes.
async fn raw(
    method: &str,
    url: String,
    bearer: Option<&str>,
    body: Option<Vec<u8>>,
    if_none_match: Option<String>,
) -> (u16, String, Option<String>, Vec<u8>) {
    let method = method.to_owned();
    let auth = bearer.map(|b| format!("Bearer {b}"));
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut resp = if method == "PUT" {
            let mut r = agent.put(&url).header("Content-Type", "image/png");
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.send(&body.unwrap_or_default()[..]).unwrap()
        } else {
            let mut r = agent.get(&url);
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            if let Some(t) = &if_none_match {
                r = r.header("If-None-Match", t);
            }
            r.call().unwrap()
        };
        let header = |resp: &ureq::http::Response<ureq::Body>, name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let code = resp.status().as_u16();
        let kind = header(&resp, "content-type").unwrap_or_default();
        let etag = header(&resp, "etag");
        let bytes = resp.body_mut().read_to_vec().unwrap_or_default();
        (code, kind, etag, bytes)
    })
    .await
    .unwrap()
}

impl Server {
    /// Upload a picture as a chatter: the status and the answer's JSON.
    async fn upload(&self, session: &str, bytes: Vec<u8>) -> (u16, Value) {
        let (code, _, _, body) = raw(
            "PUT",
            format!("{}/api/v1/chat/me/avatar", self.base),
            Some(session),
            Some(bytes),
            None,
        )
        .await;
        (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn fetch(&self, path: &str, bearer: Option<&str>) -> (u16, String, Vec<u8>) {
        let (code, kind, _, bytes) =
            raw("GET", format!("{}{path}", self.base), bearer, None, None).await;
        (code, kind, bytes)
    }

    fn avatar_file(&self, id: &str) -> PathBuf {
        self.avatars.join(format!("{id}.png"))
    }

    async fn avatar_rows(&self, account: &str) -> Vec<(String, String)> {
        sqlx::query_as::<_, (uuid::Uuid, String)>(
            "SELECT id, state FROM chat_avatar WHERE account_id = $1::uuid ORDER BY uploaded_at",
        )
        .bind(account)
        .fetch_all(&self.db)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.0.to_string(), r.1))
        .collect()
    }

    /// Make a chatter a moderator who is not staff.
    async fn make_moderator(&self, account: &str) {
        let (code, body) = self
            .moderate(json!({"op": "standing", "account": account, "standing": "moderator"}))
            .await;
        assert_eq!(code, 200, "{body}");
    }
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    (be(16), be(20))
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_uploaded_picture_is_rewritten_held_from_everyone_then_approved_for_all() {
    let s = server().await;
    let (morag, morag_id) = s.chatter("morag.fraser@mail.example", "Morag Fraser").await;
    let (hamish, hamish_id) = s
        .chatter("hamish.murray@mail.example", "Hamish Murray")
        .await;
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    assert_eq!(info["avatars"]["uploads"], true);
    assert_eq!(info["avatars"]["side"], 128);

    // A shipped avatar first: everyone sees it at once. One that is not in the set is refused.
    let (mut reader, _) = Client::open(&s, json!({"t": "hello"})).await;
    let (mut m, w) = Client::open(&s, json!({"t": "hello", "session": morag})).await;
    assert_eq!(w["me"]["avatar"], Value::Null);
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(morag.clone()),
        Some(json!({"id": "../fox"})),
    )
    .await;
    assert_eq!(code, 422, "{body}");
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(morag.clone()),
        Some(json!({"id": "fox"})),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["me"]["avatar"], json!({"kind": "stock", "id": "fox"}));
    let f = reader.until(|f| f["t"] == "avatar").await;
    assert_eq!(f["account"], morag_id.as_str());
    assert_eq!(f["avatar"], json!({"kind": "stock", "id": "fox"}));
    assert!(f["offset"].as_u64().is_some(), "a recoverable event");

    // Her own picture: 600 by 400 with a location in a text chunk. It comes back held.
    let (code, body) = s.upload(&morag, picture(600, 400, [30, 120, 200])).await;
    assert_eq!(code, 202, "{body}");
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    assert_eq!(
        body["me"]["avatar"],
        json!({"kind": "stock", "id": "fox"}),
        "others still see the fox"
    );
    let held = id.clone();
    m.until(move |f| f["t"] == "you" && f["me"]["avatar_held"] == held.as_str())
        .await;

    // What is kept is the worker's PNG: 128 pixels square, nothing but pixels.
    let kept = std::fs::read(s.avatar_file(&id)).unwrap();
    assert_eq!(png_size(&kept), (128, 128));
    assert!(!kept.windows(8).any(|w| w == b"Location"));
    assert!(!kept.windows(4).any(|w| w == b"tEXt"));
    assert_eq!(
        s.avatar_rows(&morag_id).await,
        vec![(id.clone(), "held".to_owned())]
    );

    // Held: nobody but its owner and staff can fetch it, whatever they send.
    let public = format!("/api/v1/chat/avatars/{id}");
    let staff_path = format!("/api/v1/chat/mod/avatars/{id}");
    assert_eq!(s.fetch(&public, None).await.0, 404, "anonymous");
    assert_eq!(
        s.fetch(&public, Some(&hamish)).await.0,
        404,
        "another chatter"
    );
    assert_eq!(
        s.fetch(&public, Some(&morag)).await.0,
        404,
        "even its owner, by the public address"
    );
    assert_eq!(s.fetch(&staff_path, None).await.0, 401);
    assert_eq!(
        s.fetch(&staff_path, Some(&hamish)).await.0,
        403,
        "a member is not a moderator"
    );
    assert_eq!(s.fetch(&staff_path, Some(&morag)).await.0, 403);
    assert_eq!(
        s.fetch("/api/v1/chat/me/avatar", Some(&hamish)).await.0,
        404,
        "another chatter's own picture is their own: none"
    );
    let (code, kind, bytes) = s.fetch("/api/v1/chat/me/avatar", Some(&morag)).await;
    assert_eq!((code, kind.as_str()), (200, "image/png"));
    assert_eq!(bytes, kept);
    let (code, kind, bytes) = s.fetch(&staff_path, Some(ADMIN)).await;
    assert_eq!((code, kind.as_str()), (200, "image/png"));
    assert_eq!(bytes, kept);

    // Her messages show the fox meanwhile.
    m.post("a1", "chat", "Evening from Crieff").await;
    let msg = reader.until(|f| f["t"] == "msg").await;
    assert_eq!(
        msg["m"]["author"]["avatar"],
        json!({"kind": "stock", "id": "fox"})
    );

    // Staff see it waiting, and so does a moderator who is not staff (ADR-0255), who sees the
    // picture too and is told of one that arrives.
    let staff = s.staff("Elspeth Gordon").await;
    let (mut desk, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    let waiting = w["moderation"]["avatars"].as_array().unwrap();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0]["id"], id.as_str());
    assert_eq!(waiting[0]["account"]["name"], "Morag Fraser");
    s.make_moderator(&hamish_id).await;
    let (mut hm, w) = Client::open(&s, json!({"t": "hello", "session": hamish})).await;
    assert_eq!(w["moderation"]["avatars"].as_array().unwrap().len(), 1);
    assert_eq!(w["moderation"]["avatars"][0]["id"], id.as_str());
    assert_eq!(s.fetch(&staff_path, Some(&hamish)).await.1, "image/png");
    let (_, _, seen) = s.fetch(&staff_path, Some(&hamish)).await;
    assert_eq!(seen, kept, "the moderator sees the picture waiting");

    // Approved by the moderator: every page is told, the picture is public, and a new page's
    // history has it.
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&hamish),
            json!({"op": "avatar_approve", "avatar": id}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(hm.gets(|f| f["t"] == "mod_avatar_done", 2000).await);
    let upload = json!({"kind": "upload", "id": id});
    let f = reader
        .until(|f| f["t"] == "avatar" && f["avatar"]["kind"] == "upload")
        .await;
    assert_eq!(f["avatar"], upload);
    assert!(desk.gets(|f| f["t"] == "mod_avatar_done", 2000).await);
    let you = m
        .until(|f| f["t"] == "you" && f["me"]["avatar"]["kind"] == "upload")
        .await;
    assert_eq!(you["me"]["avatar"], upload);
    assert_eq!(you["me"]["avatar_held"], Value::Null);
    let (code, kind, etag, bytes) =
        raw("GET", format!("{}{public}", s.base), None, None, None).await;
    assert_eq!((code, kind.as_str()), (200, "image/png"));
    assert_eq!(bytes, kept);
    let etag = etag.expect("an ETag");
    let (code, _, _, bytes) =
        raw("GET", format!("{}{public}", s.base), None, None, Some(etag)).await;
    assert_eq!((code, bytes.len()), (304, 0));
    let (_, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert_eq!(w["history"][0]["author"]["avatar"], upload);
    // A second approval finds nothing waiting.
    let (code, _) = s
        .moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    assert_eq!(code, 409);
    let (actions,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM chat_action WHERE station_id = $1 AND action = 'avatar_approve'",
    )
    .bind(s.station)
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(actions, 1);
    let (actor,): (String,) = sqlx::query_as(
        "SELECT actor FROM chat_action WHERE station_id = $1 AND action = 'avatar_approve'",
    )
    .bind(s.station)
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(
        actor, "Hamish Murray",
        "the moderation record names the moderator"
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn with_chat_hours_shows_the_room_is_read_only_between_shows_but_for_staff_and_moderators() {
    // ADR-0234: `hours.open = "shows"`. Between shows a chatter signs in and reads, but cannot
    // post or react, and is told why; the station's staff and the chatters it made moderators
    // can (ADR-0253), a moderator not in the private lane to the studio. The page learns when the next show starts, in the station's zone,
    // and the moment a scheduled show is on air every page is told and chatters post again.
    let s = server_with(uuid::Uuid::new_v4(), "", |c| {
        c.hours.open = weavesignal_chat::config::Open::Shows;
        c.hours.check_every_s = 1;
    })
    .await;
    sqlx::query(
        "INSERT INTO playout_settings (station_id, time_zone) VALUES ($1, 'Europe/London')",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    // Signing in stays open.
    let (session, _) = s.chatter("fiona@mail.example", "Fiona").await;
    let (moderator, moderator_id) = s.chatter("rory@mail.example", "Rory").await;
    sqlx::query("UPDATE chat_account SET standing = 'moderator' WHERE id = $1::uuid")
        .bind(&moderator_id)
        .execute(&s.db)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    let hours = &info["rules"]["hours"];
    assert_eq!(hours["shows_only"], true, "{info}");
    assert_eq!(hours["open"], false);
    assert_eq!(hours["opens_at_ms"], Value::Null, "nothing is scheduled");
    assert_eq!(hours["zone"], "Europe/London");

    let (mut c, w) = Client::open(&s, json!({"t": "hello", "session": session})).await;
    assert_eq!(w["rules"]["hours"]["open"], false, "{w}");
    let ack = c.post("a", "chat", "Is anyone there?").await;
    assert_eq!(ack["state"], "refused", "{ack}");
    assert_eq!(ack["code"], "closed");
    assert!(
        ack["words"]
            .as_str()
            .unwrap()
            .contains("read-only between shows"),
        "{ack}"
    );
    let ack = c.post("b", "studio", "A private word").await;
    assert_eq!(
        ack["code"], "closed",
        "the lane to the studio closes too: {ack}"
    );
    // ADR-0253: a chatter the station made a moderator posts in the room between shows, but the
    // private lane to the studio stays the staff's.
    let (mut m, _) = Client::open(&s, json!({"t": "hello", "session": moderator})).await;
    let ack = m.post("c", "chat", "Moderating").await;
    assert!(
        ack["id"].as_i64().is_some(),
        "a moderator posts between shows: {ack}"
    );
    let ack = m.post("c2", "studio", "A private word").await;
    assert_eq!(
        ack["code"], "closed",
        "but not to the studio's private lane: {ack}"
    );

    // The staff post; a reader with no account sees it.
    let staff = s.staff("Kettle FM").await;
    let (mut st, _) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    let ack = st.post("d", "announcement", "Back at seven.").await;
    let id = ack["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("the staff could not post: {ack}"));
    let (mut reader, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert!(
        w["history"].to_string().contains("Back at seven."),
        "a reader with no account sees the staff's message: {w}"
    );
    // A reaction is posting too: refused for a chatter, allowed for a moderator.
    c.send(json!({"t": "react", "id": id, "emoji": "👍"})).await;
    let e = c.until(|f| f["t"] == "error").await;
    assert!(e["words"].as_str().unwrap().contains("read-only"), "{e}");
    m.send(json!({"t": "react", "id": id, "emoji": "👍"})).await;
    let r = m.until(|f| f["t"] == "react").await;
    assert_eq!(r["id"], id, "{r}");

    // A show scheduled in an hour: the page is told when the chat opens.
    scheduled(&s, 1, 3600, 60).await;
    let opens = c
        .until(|f| f["t"] == "rules" && f["rules"]["hours"]["opens_at_ms"].is_i64())
        .await;
    let at = opens["rules"]["hours"]["opens_at_ms"].as_i64().unwrap();
    assert!((at - now() - 3_600_000).abs() < 5_000, "opens at {at}");
    assert_eq!(opens["rules"]["hours"]["open"], false);

    // A show on air now, by the schedule: every page is told, and chatters post again.
    scheduled(&s, 2, -60, 30).await;
    let open = reader
        .until(|f| f["t"] == "rules" && f["rules"]["hours"]["open"] == true)
        .await;
    assert_eq!(open["rules"]["hours"]["opens_at_ms"], Value::Null);
    let ack = c.post("e", "chat", "Evening all").await;
    assert!(ack["id"].as_i64().is_some(), "open during the show: {ack}");
    let ack = m.post("f", "chat", "Welcome in").await;
    assert!(ack["id"].as_i64().is_some(), "{ack}");
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_upload_meets_its_caps_and_only_a_signed_in_chatter_may_send_one() {
    let s = server().await;
    let (isla, isla_id) = s.chatter("isla.reid@mail.example", "Isla Reid").await;

    // Not a picture (an SVG is never taken), and one too small to see.
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>".to_vec();
    let (code, body) = s.upload(&isla, svg).await;
    assert_eq!(code, 422, "{body}");
    assert!(body["error"].as_str().unwrap().contains("PNG or JPEG"));
    let (code, body) = s.upload(&isla, picture(8, 8, [1, 2, 3])).await;
    assert_eq!(code, 422, "{body}");
    assert!(body["error"].as_str().unwrap().contains("too small"));
    // More pixels than the cap, in a file of a few hundred bytes: refused from its header.
    let mut huge = Vec::new();
    let mut e = png::Encoder::new(&mut huge, 5000, 5000);
    e.set_color(png::ColorType::Grayscale);
    e.set_depth(png::BitDepth::One);
    e.write_header().unwrap();
    let (code, body) = s.upload(&isla, huge).await;
    assert_eq!(code, 422, "{body}");
    assert!(body["error"].as_str().unwrap().contains("too large"));
    // A body over the byte cap never reaches the worker.
    let (code, _) = s.upload(&isla, vec![0u8; 2049 * 1024]).await;
    assert_eq!(code, 413);
    assert!(s.avatar_rows(&isla_id).await.is_empty());
    assert!(
        std::fs::read_dir(&s.avatars)
            .map(|d| d.count())
            .unwrap_or(0)
            == 0,
        "nothing refused is kept"
    );

    // No session, and a guest's: neither uploads. A guest still chooses from the set.
    let (code, _, _, _) = raw(
        "PUT",
        format!("{}/api/v1/chat/me/avatar", s.base),
        None,
        Some(picture(64, 64, [9, 9, 9])),
        None,
    )
    .await;
    assert_eq!(code, 401);
    let (_, settings) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    let mut with_guests = settings.clone();
    with_guests["guests"] = json!(true);
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/admin/settings", s.base),
        Some(ADMIN.into()),
        Some(with_guests),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    let (code, body) = s
        .post(
            "/api/v1/chat/guest",
            None,
            json!({"name": "Calum Ross", "age_ok": true, "terms_version": info["terms_version"]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let guest = body["session"].as_str().unwrap().to_owned();
    let (code, body) = s.upload(&guest, picture(64, 64, [9, 9, 9])).await;
    assert_eq!(code, 403, "{body}");
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(guest),
        Some(json!({"id": "owl"})),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["me"]["avatar"], json!({"kind": "stock", "id": "owl"}));

    // A second upload replaces the first one waiting: one row, one file.
    let (_, a) = s.upload(&isla, picture(64, 64, [200, 0, 0])).await;
    let first = a["me"]["avatar_held"].as_str().unwrap().to_owned();
    let (_, b) = s.upload(&isla, picture(64, 64, [0, 200, 0])).await;
    let second = b["me"]["avatar_held"].as_str().unwrap().to_owned();
    assert_ne!(first, second);
    assert_eq!(
        s.avatar_rows(&isla_id).await,
        vec![(second.clone(), "held".to_owned())]
    );
    assert!(!s.avatar_file(&first).exists() && s.avatar_file(&second).exists());

    // Five uploads in ten minutes, the refused ones counted too (three above), and no more.
    let (code, body) = s.upload(&isla, picture(64, 64, [0, 0, 200])).await;
    assert_eq!(code, 429, "{body}");

    // A timed-out chatter changes nothing; she can still take her own picture away.
    let (code, _) = s
        .moderate(json!({"op": "timeout", "account": isla_id, "minutes": 10, "reason": "spam"}))
        .await;
    assert_eq!(code, 200);
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(isla.clone()),
        Some(json!({"id": "owl"})),
    )
    .await;
    assert_eq!(code, 403, "{body}");
    let (code, body) = http(
        "DELETE",
        format!("{}/api/v1/chat/me/avatar", s.base),
        Some(isla.clone()),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["me"]["avatar_held"], Value::Null);
    assert!(s.avatar_rows(&isla_id).await.is_empty());
    assert!(!s.avatar_file(&second).exists());
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_refused_picture_is_deleted_and_its_owner_told_why() {
    let s = server().await;
    let (rory, rory_id) = s.chatter("rory.macleod@mail.example", "Rory MacLeod").await;
    let (mut r, _) = Client::open(&s, json!({"t": "hello", "session": rory})).await;
    let (mut reader, _) = Client::open(&s, json!({"t": "hello"})).await;
    let (_, body) = s.upload(&rory, picture(200, 200, [120, 40, 40])).await;
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    assert!(s.avatar_file(&id).exists());

    // A reason is needed, as for any decision the chatter is told about.
    let (code, _) = s
        .moderate(json!({"op": "avatar_refuse", "avatar": id}))
        .await;
    assert_eq!(code, 422);
    let (code, body) = s
        .moderate(
            json!({"op": "avatar_refuse", "avatar": id, "reason": "abuse", "note": "a slogan"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(
        body["done"]
            .as_str()
            .unwrap()
            .starts_with("Refused Rory MacLeod's picture")
    );

    // Gone: the row, the file, and every way to fetch it. Nobody else ever heard of it.
    assert!(s.avatar_rows(&rory_id).await.is_empty());
    assert!(!s.avatar_file(&id).exists());
    assert_eq!(s.fetch("/api/v1/chat/me/avatar", Some(&rory)).await.0, 404);
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/mod/avatars/{id}"), Some(ADMIN))
            .await
            .0,
        404
    );
    assert!(!reader.gets(|f| f["t"] == "avatar", 300).await);

    // He is told, with the reason and the way to ask for a review; the log has it.
    let st = r.until(|f| f["t"] == "statement").await;
    let text = st["s"]["text"].as_str().unwrap();
    assert!(
        text.contains("did not approve the picture you uploaded"),
        "{text}"
    );
    assert!(text.contains("abuse or harassment"), "{text}");
    assert!(text.contains("ask for a review within 14 days"), "{text}");
    let you = r
        .until(|f| f["t"] == "you" && f["me"]["avatar_held"].is_null())
        .await;
    assert_eq!(you["me"]["avatar"], Value::Null);
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(ADMIN)).await;
    let entry = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["action"] == "avatar_refuse")
        .expect("logged");
    assert_eq!(entry["reason"], "abuse");
    assert_eq!(entry["note"], "a slogan");
    assert_eq!(entry["account"], rory_id.as_str());
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_moderator_refuses_a_waiting_picture_and_the_record_names_them() {
    // ADR-0255: the queue is the moderators' as well as the staff's.
    let s = server().await;
    let (isla, isla_id) = s.chatter("isla.munro@mail.example", "Isla Munro").await;
    let (lewis, lewis_id) = s.chatter("lewis.grant@mail.example", "Lewis Grant").await;
    s.make_moderator(&lewis_id).await;
    let (mut i, _) = Client::open(&s, json!({"t": "hello", "session": isla})).await;
    let (mut l, _) = Client::open(&s, json!({"t": "hello", "session": lewis})).await;
    let (_, body) = s.upload(&isla, picture(200, 200, [90, 20, 20])).await;
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    let arrived = l.until(|f| f["t"] == "mod_avatar").await;
    assert_eq!(arrived["a"]["id"], id.as_str());
    assert_eq!(arrived["a"]["account"]["name"], "Isla Munro");
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&lewis),
            json!({"op": "avatar_refuse", "avatar": id, "reason": "off_topic", "note": "a logo"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(l.gets(|f| f["t"] == "mod_avatar_done", 2000).await);
    assert!(s.avatar_rows(&isla_id).await.is_empty());
    let st = i.until(|f| f["t"] == "statement").await;
    assert!(
        st["s"]["text"]
            .as_str()
            .unwrap()
            .contains("did not approve the picture you uploaded")
    );
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(&lewis)).await;
    let entry = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "avatar_refuse")
        .expect("logged");
    assert_eq!(entry["actor"], "Lewis Grant");
    assert_eq!(entry["note"], "a logo");
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn the_studios_switch_turns_uploads_on_and_off_at_once_over_the_install_setting() {
    // ADR-0259: the install setting is the default until the switch is used.
    let s = server_with(uuid::Uuid::new_v4(), "", |c| c.avatars.uploads = false).await;
    let (ruth, _) = s
        .chatter("ruth.sinclair@mail.example", "Ruth Sinclair")
        .await;
    let put = |on: Option<bool>| {
        let base = s.base.clone();
        async move {
            let (_, mut doc) = http(
                "GET",
                format!("{base}/api/v1/chat/admin/settings"),
                Some(ADMIN.into()),
                None,
            )
            .await;
            doc["avatar_uploads"] = on.map_or(Value::Null, |b| json!(b));
            http(
                "PUT",
                format!("{base}/api/v1/chat/admin/settings"),
                Some(ADMIN.into()),
                Some(doc),
            )
            .await
        }
    };
    let uploads =
        || async { s.get("/api/v1/chat/info", None).await.1["avatars"]["uploads"].clone() };
    let (mut page, w) = Client::open(&s, json!({"t": "hello", "session": ruth})).await;
    assert_eq!(w["rules"]["avatar_uploads"], false, "installed off");
    assert_eq!(uploads().await, false);
    let (code, body) = s.upload(&ruth, picture(100, 100, [1, 2, 3])).await;
    assert_eq!(code, 403, "{body}");

    // Switched on: every open page is told at once, and the upload is taken.
    let (code, saved) = put(Some(true)).await;
    assert_eq!(code, 200, "{saved}");
    assert_eq!(saved["avatar_uploads"], true);
    let f = page
        .until(|f| f["t"] == "rules" && f["rules"]["avatar_uploads"] == true)
        .await;
    assert_eq!(f["rules"]["avatar_uploads"], true);
    assert_eq!(uploads().await, true);
    let (code, body) = s.upload(&ruth, picture(100, 100, [1, 2, 3])).await;
    assert_eq!(code, 202, "{body}");

    // Switched off again: refused at once, though the picture already waiting stays.
    let (code, _) = put(Some(false)).await;
    assert_eq!(code, 200);
    page.until(|f| f["t"] == "rules" && f["rules"]["avatar_uploads"] == false)
        .await;
    let (code, body) = s.upload(&ruth, picture(100, 100, [4, 5, 6])).await;
    assert_eq!(code, 403, "{body}");
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": ruth})).await;
    assert!(w["me"]["avatar_held"].is_string(), "the waiting one stays");
    // Used and then cleared, it follows the install setting again.
    put(None).await;
    assert_eq!(uploads().await, false);
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(ADMIN)).await;
    let actions: Vec<&str> = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["action"].as_str())
        .filter(|a| a.starts_with("avatar_uploads"))
        .collect();
    assert_eq!(
        actions,
        vec!["avatar_uploads_off", "avatar_uploads_on"],
        "newest first: each switch is in the record"
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_picture_nobody_decides_on_for_thirty_days_is_deleted_and_its_owner_told() {
    // ADR-0254.
    let s = server().await;
    let mut ids = Vec::new();
    for (email, name) in [
        ("cara.baxter@mail.example", "Cara Baxter"),
        ("ewan.cairns@mail.example", "Ewan Cairns"),
        ("nina.stuart@mail.example", "Nina Stuart"),
        ("tam.imrie@mail.example", "Tam Imrie"),
    ] {
        let (session, who) = s.chatter(email, name).await;
        let (code, body) = s.upload(&session, picture(100, 100, [40, 40, 120])).await;
        assert_eq!(code, 202, "{body}");
        ids.push((
            who,
            body["me"]["avatar_held"].as_str().unwrap().to_owned(),
            session,
        ));
    }
    let staff = s.staff("Elspeth Gordon").await;
    let (mut desk, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    assert_eq!(w["moderation"]["avatars"].as_array().unwrap().len(), 4);
    let (mut c, _) = Client::open(&s, json!({"t": "hello", "session": ids[0].2})).await;

    // Cara's and Ewan's waited 31 days; Nina's 29; Tam's 40 but an evidence hold names him.
    for (i, days) in [(0, 31), (1, 31), (2, 29), (3, 40)] {
        sqlx::query("UPDATE chat_avatar SET uploaded_at = now() - make_interval(days => $2) WHERE id = $1::uuid")
            .bind(&ids[i].1)
            .bind(days)
            .execute(&s.db)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO chat_hold (id, station_id, account_id, reason, held_by) VALUES ($1, $2, $3::uuid, 'a threat', 'Elspeth Gordon')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(s.station)
    .bind(&ids[3].0)
    .execute(&s.db)
    .await
    .unwrap();

    // The chat's own retention task runs the same step when it starts; on a loaded machine its
    // first run can land after the rows above were aged, and get to some of them first.
    assert!(s.chat.expire_waiting_avatars().await.unwrap() <= 3);
    assert!(s.avatar_rows(&ids[0].0).await.is_empty());
    assert!(!s.avatar_file(&ids[0].1).exists());
    assert!(s.avatar_rows(&ids[1].0).await.is_empty());
    assert_eq!(
        s.avatar_rows(&ids[2].0).await,
        vec![(ids[2].1.clone(), "held".to_owned())],
        "29 days is not yet"
    );
    assert_eq!(
        s.avatar_rows(&ids[3].0).await,
        vec![(ids[3].1.clone(), "withheld".to_owned())],
        "under a hold it is withheld, as a refusal would"
    );
    assert!(s.avatar_file(&ids[3].1).exists());
    // The screener's queue loses two; the owner who is here is told at once.
    let mut gone = 0;
    while desk.gets(|f| f["t"] == "mod_avatar_done", 500).await {
        gone += 1;
        desk.seen.retain(|f| f["t"] != "mod_avatar_done");
        if gone == 3 {
            break;
        }
    }
    assert_eq!(gone, 3);
    let st = c.until(|f| f["t"] == "statement").await;
    let (code, body) = s
        .post(
            &format!(
                "/api/v1/chat/statements/{}/review",
                st["s"]["id"].as_str().unwrap()
            ),
            Some(&ids[0].2),
            json!({"text": "I uploaded it last month and nobody looked."}),
        )
        .await;
    assert_eq!(code, 201, "a review of the expiry notice is taken: {body}");
    let text = st["s"]["text"].as_str().unwrap();
    assert!(
        text.contains("nobody had decided on it after 30 days"),
        "{text}"
    );
    assert!(text.contains("upload it again"), "{text}");
    assert_eq!(st["s"]["automated"], true);
    // The one who was away reads it when they return (statements come with the welcome).
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": ids[1].2})).await;
    assert_eq!(w["statements"].as_array().unwrap().len(), 1, "{w}");
    // A second run finds nothing more, and the record names the automatic step.
    assert_eq!(s.chat.expire_waiting_avatars().await.unwrap(), 0);
    let (_, log) = s.get("/api/v1/chat/mod/log", Some(ADMIN)).await;
    let expired: Vec<&Value> = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "avatar_expire")
        .collect();
    assert_eq!(expired.len(), 3);
    assert_eq!(expired[0]["actor"], "the chat (automatic)");
    assert_eq!(expired[0]["automated"], true);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_approved_picture_is_removed_later_and_one_under_a_hold_is_withheld_not_served() {
    let s = server().await;
    let (ailsa, ailsa_id) = s.chatter("ailsa.kerr@mail.example", "Ailsa Kerr").await;
    let (finlay, finlay_id) = s.chatter("finlay.boyd@mail.example", "Finlay Boyd").await;
    http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(ailsa.clone()),
        Some(json!({"id": "otter"})),
    )
    .await;
    let (_, body) = s.upload(&ailsa, picture(128, 128, [10, 90, 60])).await;
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    let public = format!("/api/v1/chat/avatars/{id}");
    assert_eq!(s.fetch(&public, None).await.0, 200);
    let (mut a, _) = Client::open(&s, json!({"t": "hello", "session": ailsa})).await;
    a.post("k1", "chat", "Hello from Oban").await;
    let (mut reader, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert_eq!(w["history"][0]["author"]["avatar"]["kind"], "upload");

    // A moderator who is not staff may remove it (as they may hide a message).
    s.make_moderator(&finlay_id).await;
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&finlay),
            json!({"op": "avatar_remove", "account": ailsa_id, "reason": "off_topic"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let f = reader.until(|f| f["t"] == "avatar").await;
    assert_eq!(
        f["avatar"],
        json!({"kind": "stock", "id": "otter"}),
        "back to the otter"
    );
    assert_eq!(s.fetch(&public, None).await.0, 404);
    assert!(!s.avatar_file(&id).exists());
    assert!(s.avatar_rows(&ailsa_id).await.is_empty());
    let st = a.until(|f| f["t"] == "statement").await;
    assert!(
        st["s"]["text"]
            .as_str()
            .unwrap()
            .contains("removed your picture from its chat")
    );
    let (_, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert_eq!(
        w["history"][0]["author"]["avatar"],
        json!({"kind": "stock", "id": "otter"})
    );
    let (code, _) = s
        .moderate(json!({"op": "avatar_remove", "account": ailsa_id, "reason": "off_topic"}))
        .await;
    assert_eq!(code, 409, "nothing left to remove");

    // A picture refused as an intimate image is evidence: kept, withheld from everyone but
    // staff, under a hold; when the hold is released the sweep deletes it.
    let (_, body) = s.upload(&ailsa, picture(128, 128, [200, 150, 150])).await;
    let held = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    let (code, body) = s
        .moderate(json!({"op": "avatar_refuse", "avatar": held, "reason": "intimate_image"}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        s.avatar_rows(&ailsa_id).await,
        vec![(held.clone(), "withheld".to_owned())]
    );
    assert!(s.avatar_file(&held).exists());
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/avatars/{held}"), None)
            .await
            .0,
        404
    );
    assert_eq!(s.fetch("/api/v1/chat/me/avatar", Some(&ailsa)).await.0, 404);
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/mod/avatars/{held}"), Some(ADMIN))
            .await
            .0,
        200
    );
    let (_, me) = s.get("/api/v1/chat/me", Some(&ailsa)).await;
    assert_eq!(me["me"]["avatar_held"], Value::Null);
    // Erasing her while the hold is open keeps the evidence and nothing else.
    let (code, _) = s
        .post(
            &format!("/api/v1/chat/admin/accounts/{ailsa_id}/erase"),
            Some(ADMIN),
            json!({}),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(
        s.avatar_rows(&ailsa_id).await,
        vec![(held.clone(), "withheld".to_owned())]
    );
    assert!(s.avatar_file(&held).exists());

    let (_, holds) = s.get("/api/v1/chat/admin/holds", Some(ADMIN)).await;
    let hold = holds["holds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["account"] == ailsa_id.as_str())
        .expect("a hold names her");
    let (code, body) = s
        .moderate(json!({"op": "release_hold", "hold": hold["id"]}))
        .await;
    assert_eq!(code, 200, "{body}");
    let station = weavesignal_model::StationId::from(s.station);
    let swept = weavesignal_chat::store::sweep(&s.db, station, None, false, None, None)
        .await
        .unwrap();
    assert_eq!(swept.avatars, 1);
    assert!(s.avatar_rows(&ailsa_id).await.is_empty());
    // The file sweep leaves a file younger than ten minutes (its row may not be written yet).
    s.chat.sweep_avatar_files().await;
    assert!(s.avatar_file(&held).exists());
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(s.avatar_file(&held))
        .unwrap()
        .set_modified(old)
        .unwrap();
    s.chat.sweep_avatar_files().await;
    assert!(!s.avatar_file(&held).exists());
}

/// A picture the chat would accept, in a form with an easy-to-find marker in its bytes.
fn upload_original() -> Vec<u8> {
    picture(300, 300, [120, 60, 30])
}

impl Server {
    fn original_file(&self, id: &str) -> PathBuf {
        self.avatars.join(format!("{id}.orig"))
    }

    async fn avatar_columns(&self, id: &str) -> (String, Option<String>, bool, bool, bool) {
        sqlx::query_as(
            "SELECT state, reason, evidence IS NOT NULL,
                    COALESCE(keep_until > now() + interval '365 days', false), has_original
             FROM chat_avatar WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await
        .unwrap()
    }

    async fn actions(&self, action: &str) -> Vec<Value> {
        let (_, log) = self.get("/api/v1/chat/mod/log", Some(ADMIN)).await;
        log["log"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["action"] == action)
            .cloned()
            .collect()
    }
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_picture_refused_as_suspected_csam_is_kept_for_a_year_and_seen_only_by_the_named_person()
{
    // ADR-0257.
    use weavesignal_model::staff::Role;
    let s = server().await;
    let (_, first) = staff_signed_in(&s, "elspeth", "Elspeth Gordon", Role::Admin).await;
    let (second_id, second) = staff_signed_in(&s, "morven", "Morven Reid", Role::Admin).await;
    let (_, producer) = staff_signed_in(&s, "calum", "Calum Reid", Role::Producer).await;
    let (rory, rory_id) = s.chatter("rory.macleod@mail.example", "Rory MacLeod").await;
    let (moira, moira_id) = s.chatter("moira.dunn@mail.example", "Moira Dunn").await;
    s.make_moderator(&moira_id).await;
    let (mut r, _) = Client::open(&s, json!({"t": "hello", "session": rory})).await;

    // The upload as it arrived is kept beside the clean PNG, and only the PNG is ever served.
    let sent = upload_original();
    let (code, body) = s.upload(&rory, sent.clone()).await;
    assert_eq!(code, 202, "{body}");
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    assert_eq!(std::fs::read(s.original_file(&id)).unwrap(), sent);
    assert!(s.avatar_columns(&id).await.4, "has_original");
    let png = std::fs::read(s.avatar_file(&id)).unwrap();
    assert_ne!(png, sent);

    // Not a reason the public can give, nor one for a message.
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&moira),
            json!({"account_id": rory_id, "reason": "suspected_csam"}),
        )
        .await;
    assert_eq!(code, 422);
    let (code, _) = s
        .post(
            "/api/v1/chat/notices",
            None,
            json!({"reason": "suspected_csam", "location": "x", "detail": "y", "email": "a@b.example", "good_faith": true}),
        )
        .await;
    assert_eq!(code, 422);
    let (code, _) = s
        .moderate(json!({"op": "ban", "account": rory_id, "reason": "suspected_csam"}))
        .await;
    assert_eq!(code, 422);
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    assert!(!info["reasons"].to_string().contains("suspected_csam"));

    // A moderator refuses it with the reason.
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&moira),
            json!({"op": "avatar_refuse", "avatar": id, "reason": "suspected_csam", "note": "looks like a child"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        s.avatar_columns(&id).await,
        (
            "withheld".into(),
            Some("suspected_csam".into()),
            true,
            true,
            true
        ),
        "withheld, with evidence, for a year, with its original"
    );
    assert!(s.avatar_file(&id).exists() && s.original_file(&id).exists());
    let st = r.until(|f| f["t"] == "statement").await;
    let text = st["s"]["text"].as_str().unwrap();
    // Told less (the operator, 2026-11-10): nothing about the reason, keeping or reporting.
    assert!(
        text.contains("for breaking the station's rules on illegal content"),
        "{text}"
    );
    assert!(
        !text.contains("child") && !text.contains("at least a year"),
        "{text}"
    );
    let (_, holds) = s.get("/api/v1/chat/admin/holds", Some(ADMIN)).await;
    assert!(
        holds["holds"].to_string().contains(&rory_id),
        "an evidence hold names him"
    );

    // Nobody else can fetch it: not the public, the owner, a moderator, the station key, any staff.
    let open = format!("/api/v1/chat/avatars/{id}");
    let staff_path = format!("/api/v1/chat/mod/avatars/{id}");
    assert_eq!(s.fetch(&open, None).await.0, 404);
    assert_eq!(s.fetch("/api/v1/chat/me/avatar", Some(&rory)).await.0, 404);
    assert_eq!(s.fetch(&staff_path, Some(&moira)).await.0, 404);
    assert_eq!(s.fetch(&staff_path, Some(ADMIN)).await.0, 404);
    assert_eq!(
        s.fetch(&staff_path, Some(&first)).await.0,
        404,
        "even the named person, by the staff route"
    );

    // The evidence routes: the station key is not a person; the second admin and a producer are
    // not the one named (the first admin, by default); the first admin is.
    let list = "/api/v1/chat/evidence";
    assert_eq!(s.get(list, Some(ADMIN)).await.0, 401);
    assert_eq!(s.get(list, None).await.0, 401);
    let (code, body) = s.get(list, Some(&second)).await;
    assert_eq!(code, 403);
    assert!(
        body["error"].as_str().unwrap().contains("Elspeth Gordon"),
        "{body}"
    );
    assert_eq!(s.get(list, Some(&producer)).await.0, 403);
    assert_eq!(s.get(list, Some(&rory)).await.0, 401);
    let (code, cases) = s.get(list, Some(&first)).await;
    assert_eq!(code, 200, "{cases}");
    assert_eq!(cases["cases"][0]["id"], id.as_str());
    assert_eq!(cases["cases"][0]["account"], "Rory MacLeod");
    let picture_path = format!("/api/v1/chat/evidence/{id}/picture");
    let (code, kind, bytes) = s.fetch(&picture_path, Some(&first)).await;
    assert_eq!(
        (code, kind.as_str(), bytes),
        (200, "image/png", png.clone())
    );
    assert_eq!(s.fetch(&picture_path, Some(&second)).await.0, 403);
    assert_eq!(s.fetch(&picture_path, Some(ADMIN)).await.0, 401);

    // The staff see that something is kept and who can open it, but not the picture.
    let (code, kept) = s.get("/api/v1/chat/mod/kept-pictures", Some(&second)).await;
    assert_eq!(code, 200, "{kept}");
    assert_eq!(kept["evidence_viewer"]["name"], "Elspeth Gordon");
    assert_eq!(kept["you_are_the_evidence_viewer"], false);
    assert_eq!(kept["pictures"][0]["evidence"], true);
    assert_eq!(
        kept["pictures"][0]["reason_words"],
        "Suspected child sexual abuse material"
    );
    let (_, kept) = s.get("/api/v1/chat/mod/kept-pictures", Some(&first)).await;
    assert_eq!(kept["you_are_the_evidence_viewer"], true);

    // The export: the original, the picture as shown, the details and the moderation record.
    let (code, kind, zip) = s
        .fetch(&format!("/api/v1/chat/evidence/{id}/export"), Some(&first))
        .await;
    assert_eq!((code, kind.as_str()), (200, "application/zip"));
    let files = weavesignal_chat::zip::read_stored(&zip).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "README.txt",
            "original-upload.png",
            "picture-as-shown.png",
            "metadata.json",
            "moderation-record.json"
        ]
    );
    let file = |n: &str| &files.iter().find(|f| f.0 == n).unwrap().1;
    assert_eq!(file("original-upload.png"), &sent, "exactly as it arrived");
    assert_eq!(file("picture-as-shown.png"), &png);
    let readme = String::from_utf8(file("README.txt").clone()).unwrap();
    assert!(
        readme.contains("Do not forward, copy, print or screenshot"),
        "{readme}"
    );
    assert!(readme.contains("email address"), "says what is not in it");
    let meta: Value = serde_json::from_slice(file("metadata.json")).unwrap();
    let snap = &meta["snapshot_at_the_decision"];
    assert_eq!(snap["account"]["name"], "Rory MacLeod");
    assert_eq!(snap["account"]["id"], rory_id.as_str());
    assert_eq!(snap["decided_by"], "Moira Dunn");
    assert_eq!(snap["note"], "looks like a child");
    assert_eq!(snap["moderation"][0]["action"], "avatar_refuse");
    assert!(snap["moderation"][0]["statement_to_the_uploader"].is_string());
    assert_eq!(meta["files"]["original"]["bytes"], sent.len());
    let record: Value = serde_json::from_slice(file("moderation-record.json")).unwrap();
    assert!(record["actions"].to_string().contains("avatar_refuse"));
    assert_eq!(s.actions("evidence_export").await.len(), 1);
    assert_eq!(
        s.actions("evidence_view").await.len(),
        1,
        "looking is recorded too"
    );

    // He may ask for a review of that decision (a review is a report with its own reason).
    let (code, body) = s
        .post(
            &format!(
                "/api/v1/chat/statements/{}/review",
                st["s"]["id"].as_str().unwrap()
            ),
            Some(&rory),
            json!({"text": "It was a photograph of my niece's sports day."}),
        )
        .await;
    assert_eq!(code, 201, "{body}");

    // The year survives a released hold, an erasure and the moderation record's own sweep.
    let hold_id = holds["holds"][0]["id"].clone();
    let (code, _) = s
        .moderate(json!({"op": "release_hold", "hold": hold_id}))
        .await;
    assert_eq!(code, 200);
    let station = weavesignal_model::StationId::from(s.station);
    let (code, _) = s
        .post(
            &format!("/api/v1/chat/admin/accounts/{rory_id}/erase"),
            Some(ADMIN),
            json!({}),
        )
        .await;
    assert_eq!(code, 200);
    let swept = weavesignal_chat::store::sweep(&s.db, station, None, false, Some(0), Some(0))
        .await
        .unwrap();
    assert_eq!(swept.avatars, 0, "kept for its year");
    assert!(s.avatar_file(&id).exists() && s.original_file(&id).exists());
    assert!(
        !s.actions("avatar_refuse").await.is_empty(),
        "the record of the decision stays while the picture does"
    );
    let (_, cases) = s.get(list, Some(&first)).await;
    assert_eq!(
        cases["cases"][0]["account"], "Rory MacLeod",
        "the snapshot has the name erasure took"
    );

    // Choosing another person to see it moves the door; an unknown account cannot be chosen.
    let (_, mut settings) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    let put = |doc: Value| {
        http(
            "PUT",
            format!("{}/api/v1/chat/admin/settings", s.base),
            Some(ADMIN.into()),
            Some(doc),
        )
    };
    settings["evidence_viewer"] = json!(uuid::Uuid::new_v4());
    assert_eq!(put(settings.clone()).await.0, 422, "no such account");
    settings["evidence_viewer"] = json!(second_id);
    assert_eq!(put(settings.clone()).await.0, 200);
    assert_eq!(s.get(list, Some(&first)).await.0, 403);
    assert_eq!(s.get(list, Some(&second)).await.0, 200);
    assert_eq!(s.actions("evidence_viewer_set").await.len(), 1);

    // After the year the sweep deletes it, row and both files.
    sqlx::query("UPDATE chat_avatar SET keep_until = now() - interval '1 day' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&s.db)
        .await
        .unwrap();
    let swept = weavesignal_chat::store::sweep(&s.db, station, None, false, None, None)
        .await
        .unwrap();
    assert_eq!(swept.avatars, 1);
    for f in [s.avatar_file(&id), s.original_file(&id)] {
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }
    s.chat.sweep_avatar_files().await;
    assert!(!s.avatar_file(&id).exists() && !s.original_file(&id).exists());
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn an_approved_picture_found_out_later_is_kept_as_evidence_too() {
    // ADR-0257: it can follow approval. The original is deleted at approval (the operator,
    // 2026-11-10), so the evidence is the picture as it was shown.
    let s = server().await;
    let (ewan, ewan_id) = s.chatter("ewan.cairns@mail.example", "Ewan Cairns").await;
    let (_, body) = s.upload(&ewan, upload_original()).await;
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    let (code, _) = s
        .moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    assert_eq!(code, 200);
    assert!(
        !s.original_file(&id).exists(),
        "the original goes at approval"
    );
    assert!(!s.avatar_columns(&id).await.4, "has_original is false");
    let (code, body) = s
        .moderate(json!({"op": "avatar_remove", "account": ewan_id, "reason": "suspected_csam"}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(s.avatar_columns(&id).await.0, "withheld");
    assert!(s.avatar_columns(&id).await.2, "evidence");
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/avatars/{id}"), None).await.0,
        404
    );
    let (_, me) = s.get("/api/v1/chat/me", Some(&ewan)).await;
    assert_eq!(me["me"]["avatar"], Value::Null);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn staff_delete_a_kept_picture_at_once_with_a_reason_and_evidence_asks_to_confirm() {
    // ADR-0257: "delete crap from the server rather than just keeping it for a year".
    let s = server().await;
    let (ailsa, ailsa_id) = s.chatter("ailsa.kerr@mail.example", "Ailsa Kerr").await;
    let (finlay, finlay_id) = s.chatter("finlay.boyd@mail.example", "Finlay Boyd").await;
    s.make_moderator(&finlay_id).await;

    // An intimate image is withheld under a hold, as before. A moderator cannot delete it; staff
    // can, but must say why.
    let (_, body) = s.upload(&ailsa, picture(150, 150, [200, 150, 150])).await;
    let held = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    let (code, _) = s
        .moderate(json!({"op": "avatar_delete", "avatar": held, "why": "no"}))
        .await;
    assert_eq!(code, 409, "a waiting picture is refused, not deleted here");
    let (code, _) = s
        .moderate(json!({"op": "avatar_refuse", "avatar": held, "reason": "intimate_image"}))
        .await;
    assert_eq!(code, 200);
    let (_, kept) = s.get("/api/v1/chat/mod/kept-pictures", Some(ADMIN)).await;
    assert_eq!(kept["pictures"].as_array().unwrap().len(), 1);
    assert_eq!(kept["pictures"][0]["evidence"], false);
    assert_eq!(kept["pictures"][0]["reason"], "intimate_image");
    let (code, _) = s
        .post(
            "/api/v1/chat/mod",
            Some(&finlay),
            json!({"op": "avatar_delete", "avatar": held, "why": "rubbish"}),
        )
        .await;
    assert_eq!(code, 403, "staff only");
    let (code, _) = s
        .moderate(json!({"op": "avatar_delete", "avatar": held, "why": "  "}))
        .await;
    assert_eq!(code, 422, "say why");
    assert!(s.avatar_file(&held).exists());
    let (code, body) = s
        .moderate(json!({"op": "avatar_delete", "avatar": held, "why": "not needed: the person was blocked"}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(s.avatar_rows(&ailsa_id).await.is_empty());
    assert!(!s.avatar_file(&held).exists() && !s.original_file(&held).exists());
    let deleted = s.actions("avatar_delete").await;
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0]["reason"], "intimate_image");
    assert_eq!(deleted[0]["note"], "not needed: the person was blocked");
    let (_, kept) = s.get("/api/v1/chat/mod/kept-pictures", Some(ADMIN)).await;
    assert_eq!(kept["pictures"], json!([]));

    // Evidence of suspected CSAM asks to confirm, in the law's words; the station decides.
    let (_, body) = s.upload(&ailsa, picture(150, 150, [10, 10, 10])).await;
    let held = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    let (code, _) = s
        .moderate(json!({"op": "avatar_refuse", "avatar": held, "reason": "suspected_csam"}))
        .await;
    assert_eq!(code, 200);
    let (code, body) = s
        .moderate(
            json!({"op": "avatar_delete", "avatar": held, "why": "reported; the NCA has a copy"}),
        )
        .await;
    assert_eq!(code, 409, "{body}");
    let words = body["error"].as_str().unwrap();
    assert!(words.contains("report it and then keep it"), "{words}");
    assert!(s.avatar_file(&held).exists() && s.original_file(&held).exists());
    let (code, body) = s
        .moderate(json!({"op": "avatar_delete", "avatar": held, "why": "reported; the NCA has a copy", "confirm_evidence": true}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(!s.avatar_file(&held).exists() && !s.original_file(&held).exists());
    let deleted = s.actions("avatar_delete").await;
    assert_eq!(deleted[0]["reason"], "suspected_csam");
    assert!(
        deleted[0]["note"]
            .as_str()
            .unwrap()
            .starts_with("kept as evidence, deleted on confirmation")
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_listener_reports_a_chatters_picture_into_the_screeners_reports_and_staff_remove_it() {
    // ADR-0260.
    let s = server().await;
    let (rory, rory_id) = s.chatter("rory.macleod@mail.example", "Rory MacLeod").await;
    let (moira, _) = s.chatter("moira.dunn@mail.example", "Moira Dunn").await;
    let (_, body) = s.upload(&rory, upload_original()).await;
    let first = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": first}))
        .await;
    let staff = s.staff("Elspeth Gordon").await;
    let (mut desk, _) = Client::open(&s, json!({"t": "hello", "session": staff})).await;

    // Only the picture showing, and only with the person it belongs to.
    let report = |body: Value| s.post("/api/v1/chat/reports", Some(&moira), body);
    let (code, _) = report(json!({"avatar_id": first, "reason": "abuse"})).await;
    assert_eq!(code, 422, "a picture needs its owner named");
    let (code, _) = report(
        json!({"account_id": rory_id, "avatar_id": uuid::Uuid::new_v4(), "reason": "abuse"}),
    )
    .await;
    assert_eq!(code, 404, "no such picture showing");
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&rory),
            json!({"account_id": rory_id, "avatar_id": first, "reason": "abuse"}),
        )
        .await;
    assert_eq!(code, 422, "not their own");
    let (code, body) = report(
        json!({"account_id": rory_id, "avatar_id": first, "reason": "abuse", "detail": "a slogan"}),
    )
    .await;
    assert_eq!(code, 201, "{body}");

    // It lands in the screener's reports, marked as a picture.
    let item = desk.until(|f| f["t"] == "mod_item").await;
    assert_eq!(item["item"]["picture"], first.as_str(), "{item}");
    assert_eq!(item["item"]["account"]["name"], "Rory MacLeod");
    assert_eq!(item["item"]["reason"], "abuse");
    let item_id = item["item"]["id"].as_str().unwrap().to_owned();
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    assert_eq!(w["moderation"]["items"][0]["picture"], first.as_str());

    // A report about one of his messages is its own item and is not marked as a picture, whatever
    // else is open about him: its buttons must not close the picture's report.
    let (mut rc, _) = Client::open(&s, json!({"t": "hello", "session": rory})).await;
    let ack = rc.post("m1", "chat", "Evening all").await;
    let said = ack["id"].as_i64().unwrap();
    let (code, _) = report(json!({"message_id": said, "reason": "off_topic"})).await;
    assert_eq!(code, 201);
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    let items = w["moderation"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
    let about_message = items.iter().find(|i| i["message"].is_object()).unwrap();
    assert_eq!(about_message["picture"], Value::Null);
    let about_picture = items.iter().find(|i| i["message"].is_null()).unwrap();
    assert_eq!(about_picture["picture"], first.as_str());

    // A report of a child-safety kind opens an evidence hold on the account.
    let (code, _) =
        report(json!({"account_id": rory_id, "avatar_id": first, "reason": "child_safety"})).await;
    assert_eq!(code, 201);

    // A new picture, approved, is not the one reported: removing "the reported picture" is refused.
    let (_, body) = s.upload(&rory, picture(160, 160, [9, 99, 9])).await;
    let second = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": second}))
        .await;
    let (code, body) = s
        .moderate(json!({"op": "avatar_remove", "account": rory_id, "reason": "abuse", "report": item_id}))
        .await;
    assert_eq!(code, 409, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("different picture"),
        "{body}"
    );
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    assert_eq!(
        w["moderation"]["items"][0]["picture"],
        Value::Null,
        "no longer the picture showing"
    );
    assert!(s.avatar_file(&second).exists());

    // Withdrawing deletes it, under a hold too (the operator, 2026-11-10: as before chat-2).
    let (code, _) = http(
        "DELETE",
        format!("{}/api/v1/chat/me/avatar", s.base),
        Some(rory.clone()),
        None,
    )
    .await;
    assert_eq!(code, 200);
    let rows = s.avatar_rows(&rory_id).await;
    assert!(
        !rows.iter().any(|r| r.0 == second),
        "deleted, not kept: {rows:?}"
    );
    assert!(!s.avatar_file(&second).exists());
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/avatars/{second}"), None)
            .await
            .0,
        404
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn staff_remove_the_reported_picture_from_its_report() {
    let s = server().await;
    let (tam, tam_id) = s.chatter("tam.imrie@mail.example", "Tam Imrie").await;
    let (nina, _) = s.chatter("nina.stuart@mail.example", "Nina Stuart").await;
    let (_, body) = s.upload(&tam, upload_original()).await;
    let id = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&nina),
            json!({"account_id": tam_id, "avatar_id": id, "reason": "off_topic"}),
        )
        .await;
    assert_eq!(code, 201);
    let staff = s.staff("Elspeth Gordon").await;
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    let item = &w["moderation"]["items"][0];
    assert_eq!(item["picture"], id.as_str());
    let (code, body) = s
        .moderate(json!({"op": "avatar_remove", "account": tam_id, "reason": "off_topic", "report": item["id"]}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(s.avatar_rows(&tam_id).await.is_empty());
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    assert_eq!(
        w["moderation"]["items"],
        json!([]),
        "the report is closed with it"
    );
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/avatars/{id}"), None).await.0,
        404
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn with_chat_hours_always_nothing_closes() {
    // The default, as 1.0.0: open with nothing scheduled.
    let s = server().await;
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    assert_eq!(info["rules"]["hours"]["shows_only"], false, "{info}");
    assert_eq!(info["rules"]["hours"]["open"], true);
    let (session, _) = s.chatter("ailsa@mail.example", "Ailsa").await;
    let (mut c, _) = Client::open(&s, json!({"t": "hello", "session": session})).await;
    let ack = c.post("a", "chat", "Hello").await;
    assert!(ack["id"].as_i64().is_some(), "{ack}");
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn erasing_a_chatter_deletes_their_pictures_and_their_choice() {
    let s = server().await;
    let (kirsty, kirsty_id) = s.chatter("kirsty.shaw@mail.example", "Kirsty Shaw").await;
    http(
        "PUT",
        format!("{}/api/v1/chat/me/avatar/stock", s.base),
        Some(kirsty.clone()),
        Some(json!({"id": "penguin"})),
    )
    .await;
    let (_, body) = s.upload(&kirsty, picture(100, 100, [5, 5, 80])).await;
    let approved = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": approved}))
        .await;
    let (_, body) = s.upload(&kirsty, picture(100, 100, [80, 5, 5])).await;
    let waiting = body["me"]["avatar_held"].as_str().unwrap().to_owned();
    assert_eq!(s.avatar_rows(&kirsty_id).await.len(), 2);
    assert!(s.avatar_file(&approved).exists() && s.avatar_file(&waiting).exists());

    // Her export names both pictures.
    let (_, export) = s
        .get(
            &format!("/api/v1/chat/admin/accounts/{kirsty_id}/export"),
            Some(ADMIN),
        )
        .await;
    assert_eq!(export["avatars"].as_array().unwrap().len(), 2);
    assert_eq!(export["account"]["avatar_stock"], "penguin");

    let staff = s.staff("Elspeth Gordon").await;
    let (mut desk, w) = Client::open(&s, json!({"t": "hello", "session": staff.clone()})).await;
    assert_eq!(w["moderation"]["avatars"].as_array().unwrap().len(), 1);

    let (code, body) = s
        .post(
            &format!("/api/v1/chat/admin/accounts/{kirsty_id}/erase"),
            Some(ADMIN),
            json!({}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(s.avatar_rows(&kirsty_id).await.is_empty());
    assert!(!s.avatar_file(&approved).exists() && !s.avatar_file(&waiting).exists());
    // The open screener's queue loses the waiting one at once.
    let gone = waiting.clone();
    desk.until(move |f| f["t"] == "mod_avatar_done" && f["id"] == gone.as_str())
        .await;
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/avatars/{approved}"), None)
            .await
            .0,
        404
    );
    let (stock,): (Option<String>,) =
        sqlx::query_as("SELECT avatar_stock FROM chat_account WHERE id = $1::uuid")
            .bind(&kirsty_id)
            .fetch_one(&s.db)
            .await
            .unwrap();
    assert_eq!(stock, None);
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    assert_eq!(w["moderation"]["avatars"], json!([]));
}

// ---- pictures and documents in the chat (ADR-0262) ----

/// A chat with pictures and documents switched on, kept in a directory of its own.
async fn attachments_server() -> (Server, PathBuf) {
    attachments_server_for(uuid::Uuid::new_v4()).await
}

async fn attachments_server_for(station: uuid::Uuid) -> (Server, PathBuf) {
    let dir = std::env::temp_dir().join(format!("weavesignal-chat-attachments-{station}"));
    let d = dir.clone();
    let s = server_with(station, "", move |c| {
        c.attachments.uploads = true;
        c.attachments.dir = d;
        c.attachments.worker = Some(env!("CARGO_BIN_EXE_weavesignal-chat").into());
        c.attachments.max_document_kb = 64;
        c.attachments.max_picture_kb = 8192;
    })
    .await;
    (s, dir)
}

/// A screenshot of a document: dark strokes on white, with a text chunk as an editor leaves one.
fn screenshot(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![255u8; (width * height * 3) as usize];
    for y in 0..height {
        for x in 0..width {
            if (y / 6) % 3 == 0 && (x / 3) % 4 != 0 {
                let i = ((y * width + x) * 3) as usize;
                data[i..i + 3].copy_from_slice(&[20, 20, 30]);
            }
        }
    }
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.add_text_chunk("Location".into(), "57.1497 N, 2.0943 W".into())
        .unwrap();
    let mut w = e.write_header().unwrap();
    w.write_image_data(&data).unwrap();
    w.finish().unwrap();
    out
}

/// A ZIP whose first entry is `name`, stored, with `content`: how an OpenDocument file begins.
fn zip_first(name: &str, content: &[u8]) -> Vec<u8> {
    let mut z = b"PK\x03\x04".to_vec();
    z.extend_from_slice(&20u16.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    z.extend_from_slice(&[0; 8]);
    z.extend_from_slice(&(content.len() as u32).to_le_bytes());
    z.extend_from_slice(&(content.len() as u32).to_le_bytes());
    z.extend_from_slice(&(name.len() as u16).to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    z.extend_from_slice(name.as_bytes());
    z.extend_from_slice(content);
    z.extend_from_slice(b"PK\x03\x04content.xml and the rest");
    z
}

const PDF: &[u8] =
    b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n";

/// A response's status, headers and bytes.
async fn fetch_full(
    url: String,
    bearer: Option<String>,
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut r = agent.get(&url);
        if let Some(b) = &bearer {
            r = r.header("Authorization", &format!("Bearer {b}"));
        }
        let mut resp = r.call().unwrap();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        let code = resp.status().as_u16();
        (
            code,
            headers,
            resp.body_mut().read_to_vec().unwrap_or_default(),
        )
    })
    .await
    .unwrap()
}

impl Server {
    /// Post a message with a file: the status and the answer's JSON. The chat's own pace (a
    /// message every 0.67 s, five in ten seconds) is waited out, as a person would.
    async fn attach(&self, session: &str, name: &str, body: &str, bytes: &[u8]) -> (u16, Value) {
        for _ in 0..12 {
            let (code, v) = self.attach_once(session, name, body, bytes).await;
            if code == 429 && v["error"].as_str().is_some_and(|e| e.contains("Slow down")) {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                continue;
            }
            return (code, v);
        }
        panic!("still too fast after waiting");
    }

    async fn attach_once(
        &self,
        session: &str,
        name: &str,
        body: &str,
        bytes: &[u8],
    ) -> (u16, Value) {
        let url = format!(
            "{}/api/v1/chat/attachments?name={}&body={}&nonce=n-{}",
            self.base,
            urlencode(name),
            urlencode(body),
            uuid::Uuid::new_v4().simple()
        );
        let auth = format!("Bearer {session}");
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into();
            let mut resp = agent
                .post(&url)
                .header("Authorization", &auth)
                .header("Content-Type", "application/octet-stream")
                .send(&bytes[..])
                .unwrap();
            let code = resp.status().as_u16();
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            (code, serde_json::from_str(&text).unwrap_or(Value::Null))
        })
        .await
        .unwrap()
    }

    /// A chatter the station trusts (the standing that may post links, and attach).
    async fn trusted(&self, email: &str, name: &str) -> (String, String) {
        let (session, id) = self.chatter(email, name).await;
        let (code, body) = self
            .moderate(json!({"op": "standing", "account": id, "standing": "trusted"}))
            .await;
        assert_eq!(code, 200, "{body}");
        (session, id)
    }

    async fn attachment_row(&self, id: &str) -> Option<(String, String, Option<String>, bool)> {
        sqlx::query_as(
            "SELECT state, kind, mime, evidence IS NOT NULL FROM chat_avatar WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .unwrap()
    }

    async fn message_state(&self, id: i64) -> Option<String> {
        sqlx::query_as::<_, (String,)>(
            "SELECT state FROM chat_message WHERE station_id = $1 AND id = $2",
        )
        .bind(self.station)
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .unwrap()
        .map(|r| r.0)
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn files_of(dir: &std::path::Path, id: &str) -> [bool; 3] {
    ["full", "thumb", "orig"].map(|e| dir.join(format!("{id}.{e}")).exists())
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_trusted_chatters_picture_is_rewritten_held_from_everyone_then_approved_for_all() {
    let (s, dir) = attachments_server().await;
    let (ailsa, ailsa_id) = s.trusted("ailsa.grant@mail.example", "Ailsa Grant").await;
    let (hamish, _) = s
        .chatter("hamish.murray@mail.example", "Hamish Murray")
        .await;
    let staff = s.staff("Elspeth Gordon").await;
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    assert_eq!(info["attachments"]["uploads"], true);
    assert_eq!(info["rules"]["attachments"], true);
    let (mut listener, _) = Client::open(&s, json!({"t": "hello", "session": hamish})).await;
    let (mut author, _) = Client::open(&s, json!({"t": "hello", "session": ailsa.clone()})).await;
    let (mut desk, _) = Client::open(&s, json!({"t": "hello", "session": staff.clone()})).await;

    // A 2400 by 1350 screenshot with a location in it.
    let sent = screenshot(2400, 1350);
    let (code, body) = s
        .attach(
            &ailsa,
            "C:\\Users\\Ailsa\\running order.png",
            "Tonight's running order",
            &sent,
        )
        .await;
    assert_eq!(code, 202, "{body}");
    let mid = body["id"].as_i64().unwrap();
    let at = body["attachment"].clone();
    let id = at["id"].as_str().unwrap().to_owned();
    assert_eq!(at["kind"], "picture");
    assert_eq!(at["name"], "running order.png", "the name without its path");
    assert_eq!(
        (at["width"].as_u64(), at["height"].as_u64()),
        (Some(1600), Some(900))
    );
    assert_eq!(
        (at["thumb_width"].as_u64(), at["thumb_height"].as_u64()),
        (Some(480), Some(270))
    );
    assert_eq!(at["mime"], "image/png", "a screenshot stays lossless");
    assert_eq!(files_of(&dir, &id), [true, true, true]);
    assert_eq!(std::fs::read(dir.join(format!("{id}.orig"))).unwrap(), sent);
    let full = std::fs::read(dir.join(format!("{id}.full"))).unwrap();
    assert!(
        !full.windows(8).any(|w| w == b"Location"),
        "nothing but pixels"
    );
    assert_eq!(png_size(&full), (1600, 900));

    // The author sees it waiting; the room does not; the queue has it, with the message.
    let mine = author.until(|f| f["t"] == "mine").await;
    assert_eq!(mine["m"]["state"], "held");
    assert_eq!(mine["m"]["attachment"]["id"], id.as_str());
    let queued = desk.until(|f| f["t"] == "mod_avatar").await;
    assert_eq!(queued["a"]["kind"], "picture");
    assert_eq!(queued["a"]["message"]["body"], "Tonight's running order");
    assert_eq!(queued["a"]["account"]["id"], ailsa_id.as_str());
    assert!(!listener.gets(|f| f["t"] == "msg", 300).await);
    let pic = format!("/api/v1/chat/pictures/{id}");
    let thumb = format!("{pic}/thumb");
    assert_eq!(
        s.fetch(&pic, None).await.0,
        404,
        "not served before approval"
    );
    assert_eq!(s.fetch(&thumb, None).await.0, 404);
    assert_eq!(
        s.fetch(&pic, Some(&hamish)).await.0,
        404,
        "nor to another chatter"
    );
    assert_eq!(
        s.fetch(&pic, Some(&ailsa)).await.0,
        200,
        "its author sees it"
    );
    assert_eq!(
        s.fetch(&pic, Some(ADMIN)).await.0,
        200,
        "the studio sees it"
    );

    // Release-all of the hold for new chatters does not release it, and nor does a restart.
    let (code, _) = s.moderate(json!({"op": "release_all"})).await;
    assert!(code == 200 || code == 409);
    assert_eq!(s.message_state(mid).await.as_deref(), Some("held"));
    let again = server_with(s.station, "", {
        let d = dir.clone();
        move |c| {
            c.attachments.uploads = true;
            c.attachments.dir = d;
        }
    })
    .await;
    let (_, w) = Client::open(&again, json!({"t": "hello", "session": staff.clone()})).await;
    assert_eq!(
        w["moderation"]["held"],
        json!([]),
        "not in the hold for new chatters"
    );
    assert_eq!(
        w["moderation"]["avatars"][0]["id"],
        id.as_str(),
        "in the pictures' queue: {}",
        w["moderation"]
    );
    let (_, w) = Client::open(&again, json!({"t": "hello"})).await;
    assert!(!w["history"].to_string().contains(&id));

    // Approved: everyone sees the message with it, and the files are served.
    let (code, body) = s
        .moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    assert_eq!(code, 200, "{body}");
    let shown = listener.until(|f| f["t"] == "msg").await;
    assert_eq!(shown["m"]["id"].as_i64(), Some(mid));
    assert_eq!(shown["m"]["attachment"]["id"], id.as_str());
    assert_eq!(shown["m"]["body"], "Tonight's running order");
    author.until(|f| f["t"] == "released").await;
    desk.until(|f| f["t"] == "mod_avatar_done").await;
    assert_eq!(
        files_of(&dir, &id),
        [true, true, false],
        "approved: the original goes"
    );
    let (code, h, bytes) = fetch_full(format!("{}{pic}", s.base), None).await;
    assert_eq!(code, 200);
    assert_eq!(bytes, full);
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["content-security-policy"], "default-src 'none'; sandbox");
    assert_eq!(h["content-disposition"], "inline");
    let (code, _, tb) = fetch_full(format!("{}{thumb}", s.base), None).await;
    assert_eq!((code, png_size(&tb)), (200, (480, 270)));
    // A late page sees it in the history.
    let (_, w) = Client::open(&s, json!({"t": "hello"})).await;
    assert!(w["history"].to_string().contains(&id));
    assert_eq!(s.actions("attachment_approve").await.len(), 1);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn who_may_attach_and_what_is_taken_is_decided_by_the_bytes() {
    let (s, dir) = attachments_server().await;
    let (member, _) = s.chatter("lesley.kerr@mail.example", "Lesley Kerr").await;
    let (ailsa, _) = s.trusted("ailsa.grant@mail.example", "Ailsa Grant").await;
    let staff = s.staff("Elspeth Gordon").await;

    // A member who is not trusted, and a guest, may not.
    let (code, body) = s.attach(&member, "a.pdf", "", PDF).await;
    assert_eq!(code, 403, "{body}");
    assert!(body["error"].as_str().unwrap().contains("trusted"));
    let (code, body) = s
        .post(
            "/api/v1/chat/guest",
            None,
            json!({"name": "Guest Iona", "age_ok": true}),
        )
        .await;
    if code == 200 {
        let g = body["session"].as_str().unwrap().to_owned();
        assert_eq!(s.attach(&g, "a.pdf", "", PDF).await.0, 403);
    }

    // A PDF named .png is a PDF: a document, offered as a download, never a picture.
    let (code, body) = s.attach(&ailsa, "holiday.png", "", PDF).await;
    assert_eq!(code, 202, "{body}");
    let pdf = body["attachment"]["id"].as_str().unwrap().to_owned();
    assert_eq!(body["attachment"]["kind"], "document");
    assert_eq!(body["attachment"]["mime"], "application/pdf");
    assert_eq!(body["attachment"]["label"], "PDF");
    assert_eq!(
        files_of(&dir, &pdf),
        [false, false, true],
        "passed on unchanged"
    );
    assert_eq!(std::fs::read(dir.join(format!("{pdf}.orig"))).unwrap(), PDF);
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/pictures/{pdf}"), Some(ADMIN))
            .await
            .0,
        404
    );

    // An OpenDocument text and spreadsheet, by the zip's mimetype; Markdown by its name only.
    let odt = zip_first("mimetype", b"application/vnd.oasis.opendocument.text");
    let (code, body) = s.attach(&ailsa, "minutes.odt", "", &odt).await;
    assert_eq!(
        (code, body["attachment"]["label"].as_str()),
        (202, Some("OpenDocument text")),
        "{body}"
    );
    let ods = zip_first(
        "mimetype",
        b"application/vnd.oasis.opendocument.spreadsheet",
    );
    let (code, body) = s.attach(&ailsa, "rota.ods", "", &ods).await;
    assert_eq!(
        (code, body["attachment"]["label"].as_str()),
        (202, Some("OpenDocument spreadsheet")),
        "{body}"
    );
    let (code, body) = s
        .attach(&ailsa, "notes.md", "", b"# Notes\n\n- the Tay bridge")
        .await;
    assert_eq!(
        (code, body["attachment"]["mime"].as_str()),
        (202, Some("text/markdown; charset=utf-8")),
        "{body}"
    );

    // A zip that is not OpenDocument, an SVG, a page of HTML, a program: refused.
    for (name, bytes) in [
        ("report.docx", zip_first("[Content_Types].xml", b"<Types/>")),
        (
            "slides.odp",
            zip_first(
                "mimetype",
                b"application/vnd.oasis.opendocument.presentation",
            ),
        ),
        (
            "logo.svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>".to_vec(),
        ),
        (
            "page.txt",
            b"<!DOCTYPE html><html><script>alert(1)</script></html>".to_vec(),
        ),
        ("setup.pdf", b"MZ\x90\0\x03\0\0\0".to_vec()),
        ("anim.gif", b"GIF89a\x01\0\x01\0".to_vec()),
    ] {
        let (code, body) = s.attach(&ailsa, name, "", &bytes).await;
        assert_eq!(code, 422, "{name}: {body}");
    }

    // A polyglot: a PNG with a page of script after it is a picture, and the script is gone.
    let mut poly = screenshot(64, 64);
    poly.extend_from_slice(b"<html><script>alert(document.cookie)</script></html>");
    let (code, body) = s.attach(&ailsa, "poly.html", "", &poly).await;
    assert_eq!(code, 202, "{body}");
    let pid = body["attachment"]["id"].as_str().unwrap().to_owned();
    assert_eq!(body["attachment"]["kind"], "picture");
    for f in ["full", "thumb"] {
        let b = std::fs::read(dir.join(format!("{pid}.{f}"))).unwrap();
        assert!(!b.windows(7).any(|w| w == b"<script"), "{f}");
    }
    // A PDF that carries HTML is a PDF, sent as one, only as a download.
    let mut pdf_html = PDF.to_vec();
    pdf_html.extend_from_slice(b"<html><script>alert(1)</script></html>");
    let (code, body) = s.attach(&ailsa, "page.html", "", &pdf_html).await;
    assert_eq!(code, 202, "{body}");
    let ph = body["attachment"]["id"].as_str().unwrap().to_owned();
    s.moderate(json!({"op": "avatar_approve", "avatar": ph}))
        .await;
    let (code, h, bytes) = fetch_full(format!("{}/api/v1/chat/files/{ph}", s.base), None).await;
    assert_eq!(code, 200);
    assert_eq!(bytes, pdf_html, "unchanged");
    assert_eq!(h["content-type"], "application/pdf");
    let short = &ph.replace('-', "")[..8];
    assert_eq!(
        h["content-disposition"],
        format!("attachment; filename=\"chat-document-{short}.pdf\"")
    );
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["content-security-policy"], "default-src 'none'; sandbox");

    // WebP in, through the sandboxed worker; a photograph comes out as a JPEG.
    let fox = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/ui/avatars/fox.webp"
    ))
    .unwrap();
    let (code, body) = s.attach(&ailsa, "fox.webp", "", &fox).await;
    assert_eq!(code, 202, "{body}");
    assert_eq!(body["attachment"]["kind"], "picture");
    assert_eq!(
        body["attachment"]["mime"], "image/png",
        "a small drawing stays lossless"
    );
    let photo = {
        let (w, h) = (1800u32, 1200u32);
        let mut x: u32 = 0x9e37_79b9;
        let data: Vec<u8> = (0..w * h * 3)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                ((i / 3 % w) / 8 + (x & 63)) as u8
            })
            .collect();
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&data).unwrap();
        wr.finish().unwrap();
        out
    };
    let (code, body) = s.attach(&ailsa, "harbour.png", "", &photo).await;
    assert_eq!(code, 202, "{body}");
    assert_eq!(
        body["attachment"]["mime"], "image/jpeg",
        "a photograph becomes a JPEG"
    );
    let jid = body["attachment"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        (
            body["attachment"]["width"].as_u64(),
            body["attachment"]["height"].as_u64()
        ),
        (Some(1600), Some(1066))
    );
    s.moderate(json!({"op": "avatar_approve", "avatar": jid}))
        .await;
    let (code, h, bytes) = fetch_full(format!("{}/api/v1/chat/pictures/{jid}", s.base), None).await;
    assert_eq!((code, h["content-type"].as_str()), (200, "image/jpeg"));
    assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));

    // The caps: a document over max_document_kb (64 here), a picture too large in pixels.
    let mut big = PDF.to_vec();
    big.resize(65 * 1024, b' ');
    let (code, body) = s.attach(&ailsa, "big.pdf", "", &big).await;
    assert_eq!(code, 422, "{body}");
    assert!(body["error"].as_str().unwrap().contains("larger than"));
    let mut huge = Vec::new();
    {
        let mut e = png::Encoder::new(&mut huge, 9000, 9000);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::One);
        e.write_header().unwrap();
    }
    let (code, body) = s.attach(&ailsa, "huge.png", "", &huge).await;
    assert_eq!(code, 422, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("too large"),
        "{body}"
    );
    let (code, _) = s.attach(&ailsa, "empty.txt", "", b"").await;
    assert!(code == 422 || code == 400);

    // The studio's switch turns it off at once, over the install setting.
    let (_, settings) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    let mut off = settings.clone();
    off["attachments"] = json!(false);
    let (code, body) = http(
        "PUT",
        format!("{}/api/v1/chat/admin/settings", s.base),
        Some(ADMIN.into()),
        Some(off),
    )
    .await;
    assert!(code == 200 || code == 204, "{body}");
    let (code, _) = s.attach(&ailsa, "a.pdf", "", PDF).await;
    assert_eq!(code, 403);
    let (_, info) = s.get("/api/v1/chat/info", None).await;
    assert_eq!(info["rules"]["attachments"], false);
    assert_eq!(s.actions("attachments_off").await.len(), 1);
    let _ = staff;
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_refused_attachment_is_deleted_and_its_message_never_shown() {
    let (s, dir) = attachments_server().await;
    let (ailsa, ailsa_id) = s.trusted("ailsa.grant@mail.example", "Ailsa Grant").await;
    let (moira, moira_id) = s.trusted("moira.dunn@mail.example", "Moira Dunn").await;
    s.make_moderator(&moira_id).await;
    let (mut author, _) = Client::open(&s, json!({"t": "hello", "session": ailsa.clone()})).await;
    let (code, body) = s
        .attach(&ailsa, "flyer.png", "Gig on Friday", &screenshot(300, 200))
        .await;
    assert_eq!(code, 202, "{body}");
    let id = body["attachment"]["id"].as_str().unwrap().to_owned();
    let mid = body["id"].as_i64().unwrap();

    // A moderator sees it, and can fetch it, but cannot approve their own.
    let (code, mine) = s.attach(&moira, "mine.pdf", "", PDF).await;
    assert_eq!(code, 202, "{mine}");
    let own = mine["attachment"]["id"].as_str().unwrap().to_owned();
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&moira),
            json!({"op": "avatar_approve", "avatar": own}),
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/pictures/{id}"), Some(&moira))
            .await
            .0,
        200
    );

    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&moira),
            json!({"op": "avatar_refuse", "avatar": id, "reason": "spam", "note": "advert"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(s.attachment_row(&id).await, None, "deleted, row ...");
    assert_eq!(files_of(&dir, &id), [false, false, false], "... and files");
    assert_eq!(s.message_state(mid).await.as_deref(), Some("hidden"));
    let r = author.until(|f| f["t"] == "rejected").await;
    assert_eq!(r["ids"], json!([mid]));
    let st = author.until(|f| f["t"] == "statement").await;
    let text = st["s"]["text"].as_str().unwrap();
    assert!(
        text.contains("did not show the picture you posted"),
        "{text}"
    );
    let log = s.actions("attachment_refuse").await;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["actor"], "Moira Dunn");
    let _ = ailsa_id;
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn suspected_csam_on_a_posted_picture_or_document_keeps_the_evidence_and_exports_it() {
    use weavesignal_model::staff::Role;
    let (s, dir) = attachments_server().await;
    let (_, first) = staff_signed_in(&s, "elspeth", "Elspeth Gordon", Role::Admin).await;
    let (_, second) = staff_signed_in(&s, "morven", "Morven Reid", Role::Admin).await;
    let (rory, rory_id) = s.trusted("rory.macleod@mail.example", "Rory MacLeod").await;
    let sent = screenshot(400, 300);
    let (_, body) = s.attach(&rory, "IMG_0001.png", "look", &sent).await;
    let pic = body["attachment"]["id"].as_str().unwrap().to_owned();
    let (_, body) = s.attach(&rory, "file.pdf", "and this", PDF).await;
    let doc = body["attachment"]["id"].as_str().unwrap().to_owned();
    for id in [&pic, &doc] {
        let (code, body) = s
            .moderate(json!({"op": "avatar_refuse", "avatar": id, "reason": "suspected_csam"}))
            .await;
        assert_eq!(code, 200, "{body}");
        let (state, _, _, evidence) = s.attachment_row(id).await.unwrap();
        assert_eq!((state.as_str(), evidence), ("withheld", true));
        // Served to nobody by the ordinary routes, the studio included.
        for path in [
            format!("/api/v1/chat/pictures/{id}"),
            format!("/api/v1/chat/files/{id}"),
        ] {
            assert_eq!(s.fetch(&path, None).await.0, 404);
            assert_eq!(s.fetch(&path, Some(ADMIN)).await.0, 404);
            assert_eq!(s.fetch(&path, Some(&first)).await.0, 404);
            assert_eq!(s.fetch(&path, Some(&rory)).await.0, 404);
        }
    }
    assert_eq!(files_of(&dir, &pic), [true, true, true]);
    assert_eq!(files_of(&dir, &doc), [false, false, true]);

    // The named person exports each: the original, what was shown, and the message.
    let unzip = |zip: &[u8]| -> Vec<String> {
        // Stored entries: each local header is followed by its name.
        let mut names = vec![];
        let mut i = 0;
        while i + 30 <= zip.len() && &zip[i..i + 4] == b"PK\x03\x04" {
            let n = u16::from_le_bytes([zip[i + 26], zip[i + 27]]) as usize;
            let size =
                u32::from_le_bytes([zip[i + 18], zip[i + 19], zip[i + 20], zip[i + 21]]) as usize;
            names.push(String::from_utf8_lossy(&zip[i + 30..i + 30 + n]).into_owned());
            i += 30 + n + size;
        }
        names
    };
    let (code, _, zip) = s
        .fetch(&format!("/api/v1/chat/evidence/{pic}/export"), Some(&first))
        .await;
    assert_eq!(code, 200);
    let names = unzip(&zip);
    assert!(
        names.contains(&"original-upload.png".to_owned()),
        "{names:?}"
    );
    assert!(
        names.contains(&"picture-as-shown.png".to_owned()),
        "{names:?}"
    );
    assert!(
        zip.windows(4).any(|w| w == b"look"),
        "the message's words are in the snapshot"
    );
    assert!(
        zip.windows(sent.len()).any(|w| w == sent),
        "the original as it arrived"
    );
    let (code, _, zip) = s
        .fetch(&format!("/api/v1/chat/evidence/{doc}/export"), Some(&first))
        .await;
    assert_eq!(code, 200);
    let names = unzip(&zip);
    assert!(
        names.contains(&"original-upload.pdf".to_owned()),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("picture-as-shown")),
        "{names:?}"
    );
    assert!(zip.windows(PDF.len()).any(|w| w == PDF));
    assert_eq!(
        s.fetch(
            &format!("/api/v1/chat/evidence/{doc}/export"),
            Some(&second)
        )
        .await
        .0,
        403
    );
    // The named person may view the document only as a download.
    let (code, h, _) = fetch_full(
        format!("{}/api/v1/chat/evidence/{doc}/picture", s.base),
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, 200);
    assert!(h["content-disposition"].starts_with("attachment"));

    // The kept list names them; deleting one kept as evidence asks to confirm.
    let (_, kept) = s.get("/api/v1/chat/mod/kept-pictures", Some(&second)).await;
    let kinds: Vec<&str> = kept["pictures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"picture") && kinds.contains(&"document"),
        "{kinds:?}"
    );
    // Only the named person or an admin may delete one kept as evidence (the operator,
    // 2026-11-10): a producer may not, even confirming.
    let (_, producer) = staff_signed_in(&s, "calum", "Calum Reid", Role::Producer).await;
    let (code, body) = s
        .post(
            "/api/v1/chat/mod",
            Some(&producer),
            json!({"op": "avatar_delete", "avatar": doc, "why": "tidying", "confirm_evidence": true}),
        )
        .await;
    assert_eq!(code, 403, "{body}");
    let (code, _) = s
        .moderate(json!({"op": "avatar_delete", "avatar": doc, "why": "reported to the NCA"}))
        .await;
    assert_eq!(code, 409);
    let (code, body) = s
        .moderate(json!({"op": "avatar_delete", "avatar": doc, "why": "reported to the NCA", "confirm_evidence": true}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(s.attachment_row(&doc).await, None);
    assert_eq!(files_of(&dir, &doc), [false, false, false]);

    // The year survives the message's own retention, and an erasure.
    let station = weavesignal_model::StationId::from(s.station);
    sqlx::query(
        "UPDATE chat_message SET server_at = now() - interval '400 days' WHERE station_id = $1",
    )
    .bind(s.station)
    .execute(&s.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE chat_avatar SET uploaded_at = now() - interval '400 days' WHERE id = $1::uuid",
    )
    .bind(&pic)
    .execute(&s.db)
    .await
    .unwrap();
    weavesignal_chat::store::sweep(&s.db, station, Some(3600), false, None, None)
        .await
        .unwrap();
    assert!(
        s.attachment_row(&pic).await.is_some(),
        "kept past its message"
    );
    s.post(
        &format!("/api/v1/chat/admin/accounts/{rory_id}/erase"),
        Some(ADMIN),
        json!({}),
    )
    .await;
    assert!(
        s.attachment_row(&pic).await.is_some(),
        "kept past an erasure"
    );
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn report_this_on_a_posted_picture_and_staff_remove_it_with_its_message() {
    let (s, dir) = attachments_server().await;
    let (ailsa, _) = s.trusted("ailsa.grant@mail.example", "Ailsa Grant").await;
    let (hamish, _) = s
        .chatter("hamish.murray@mail.example", "Hamish Murray")
        .await;
    let (mut listener, _) =
        Client::open(&s, json!({"t": "hello", "session": hamish.clone()})).await;
    let (_, body) = s
        .attach(&ailsa, "map.png", "Where we are", &screenshot(500, 400))
        .await;
    let id = body["attachment"]["id"].as_str().unwrap().to_owned();
    let mid = body["id"].as_i64().unwrap();
    s.moderate(json!({"op": "avatar_approve", "avatar": id}))
        .await;
    listener.until(|f| f["t"] == "msg").await;

    // A wrong pairing is refused; the right one is a report about the message's picture.
    let (code, _) = s
        .post(
            "/api/v1/chat/reports",
            Some(&hamish),
            json!({"attachment_id": id, "reason": "abuse"}),
        )
        .await;
    assert_eq!(code, 422);
    let (code, body) = s
        .post(
            "/api/v1/chat/reports",
            Some(&hamish),
            json!({"message_id": mid, "attachment_id": id, "reason": "abuse", "detail": "someone's house"}),
        )
        .await;
    assert_eq!(code, 201, "{body}");
    let report = body["id"].as_str().unwrap().to_owned();
    let staff = s.staff("Elspeth Gordon").await;
    let (_, w) = Client::open(&s, json!({"t": "hello", "session": staff})).await;
    let item = w["moderation"]["items"][0].clone();
    assert_eq!(item["attachment"], id.as_str(), "{item}");
    assert_eq!(item["message"]["attachment"]["id"], id.as_str());

    let (code, body) = s
        .moderate(json!({"op": "attachment_remove", "attachment": id, "reason": "abuse", "report": report}))
        .await;
    assert_eq!(code, 200, "{body}");
    let hidden = listener.until(|f| f["t"] == "hide").await;
    assert_eq!(hidden["ids"], json!([mid]));
    assert_eq!(s.message_state(mid).await.as_deref(), Some("hidden"));
    assert_eq!(s.attachment_row(&id).await, None);
    assert_eq!(files_of(&dir, &id), [false, false, false]);
    assert_eq!(
        s.fetch(&format!("/api/v1/chat/pictures/{id}"), None)
            .await
            .0,
        404
    );
    let (_, w) = Client::open(
        &s,
        json!({"t": "hello", "session": s.staff("Elspeth Gordon").await}),
    )
    .await;
    assert_eq!(w["moderation"]["items"], json!([]), "the report is closed");
    assert_eq!(s.actions("attachment_remove").await.len(), 1);
    // Removing again finds nothing to remove.
    let (code, _) = s
        .moderate(json!({"op": "attachment_remove", "attachment": id, "reason": "abuse"}))
        .await;
    assert_eq!(code, 409);
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn attachments_go_with_their_message_by_hide_retention_waiting_and_erasure() {
    let (s, dir) = attachments_server().await;
    let station = weavesignal_model::StationId::from(s.station);
    let (ailsa, ailsa_id) = s.trusted("ailsa.grant@mail.example", "Ailsa Grant").await;
    let (mut author, _) = Client::open(&s, json!({"t": "hello", "session": ailsa.clone()})).await;
    let post = |name: &'static str| {
        let s = &s;
        let ailsa = ailsa.clone();
        async move {
            let (code, body) = s.attach(&ailsa, name, "", &screenshot(200, 100)).await;
            assert_eq!(code, 202, "{body}");
            (
                body["attachment"]["id"].as_str().unwrap().to_owned(),
                body["id"].as_i64().unwrap(),
            )
        }
    };

    // Hide: a moderator hides a shown message, and its picture goes with it.
    let (hid, hmid) = post("a.png").await;
    s.moderate(json!({"op": "avatar_approve", "avatar": hid}))
        .await;
    let (code, body) = s
        .moderate(json!({"op": "hide", "id": hmid, "reason": "spam"}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(s.attachment_row(&hid).await, None);
    assert_eq!(files_of(&dir, &hid), [false, false, false]);

    // Retention: the message goes, and its picture after it, files and all.
    let (rid, rmid) = post("b.png").await;
    s.moderate(json!({"op": "avatar_approve", "avatar": rid}))
        .await;
    sqlx::query("UPDATE chat_message SET server_at = now() - interval '2 days' WHERE station_id = $1 AND id = $2")
        .bind(s.station)
        .bind(rmid)
        .execute(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_avatar SET uploaded_at = now() - interval '2 days' WHERE id = $1::uuid",
    )
    .bind(&rid)
    .execute(&s.db)
    .await
    .unwrap();
    let swept = weavesignal_chat::store::sweep(&s.db, station, Some(3600), false, None, None)
        .await
        .unwrap();
    assert_eq!(swept.attachments, 1, "{swept:?}");
    assert_eq!(s.attachment_row(&rid).await, None);
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    for f in ["full", "thumb"] {
        std::fs::File::options()
            .write(true)
            .open(dir.join(format!("{rid}.{f}")))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    s.chat.sweep_avatar_files().await;
    assert_eq!(files_of(&dir, &rid), [false, false, false]);

    // Retention leaves a waiting one to the 30-day wait, and runs from approval for a shown one.
    let (lid, lmid) = post("late.png").await;
    sqlx::query("UPDATE chat_message SET server_at = now() - interval '7 hours' WHERE station_id = $1 AND id = $2")
        .bind(s.station)
        .bind(lmid)
        .execute(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_avatar SET uploaded_at = now() - interval '7 hours' WHERE id = $1::uuid",
    )
    .bind(&lid)
    .execute(&s.db)
    .await
    .unwrap();
    weavesignal_chat::store::sweep(&s.db, station, Some(6 * 3600), false, None, None)
        .await
        .unwrap();
    assert_eq!(
        s.message_state(lmid).await.as_deref(),
        Some("held"),
        "still waiting"
    );
    assert!(s.attachment_row(&lid).await.is_some());
    s.moderate(json!({"op": "avatar_approve", "avatar": lid}))
        .await;
    weavesignal_chat::store::sweep(&s.db, station, Some(6 * 3600), false, None, None)
        .await
        .unwrap();
    assert_eq!(
        s.message_state(lmid).await.as_deref(),
        Some("visible"),
        "retention runs from approval"
    );
    assert!(s.attachment_row(&lid).await.is_some());

    // Waiting thirty days: deleted, its message hidden, its author told.
    let (wid, wmid) = post("c.png").await;
    sqlx::query(
        "UPDATE chat_avatar SET uploaded_at = now() - interval '31 days' WHERE id = $1::uuid",
    )
    .bind(&wid)
    .execute(&s.db)
    .await
    .unwrap();
    assert_eq!(s.chat.expire_waiting_avatars().await.unwrap(), 1);
    assert_eq!(s.attachment_row(&wid).await, None);
    assert_eq!(s.message_state(wmid).await.as_deref(), Some("hidden"));
    author
        .until(|f| {
            f["t"] == "statement"
                && f["s"]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("deleted the picture you posted"))
        })
        .await;
    assert_eq!(s.actions("attachment_expire").await.len(), 1);

    // Erasure: one waiting, one shown, both gone with their files.
    let (e1, _) = post("d.png").await;
    let (e2, _) = post("e.pdf").await;
    s.moderate(json!({"op": "avatar_approve", "avatar": e2}))
        .await;
    let (_, export) = s
        .get(
            &format!("/api/v1/chat/admin/accounts/{ailsa_id}/export"),
            Some(ADMIN),
        )
        .await;
    assert_eq!(
        export["attachments"].as_array().unwrap().len(),
        3,
        "{export}"
    );
    let (code, _) = s
        .post(
            &format!("/api/v1/chat/admin/accounts/{ailsa_id}/erase"),
            Some(ADMIN),
            json!({}),
        )
        .await;
    assert_eq!(code, 200);
    for id in [&e1, &e2, &lid] {
        assert_eq!(s.attachment_row(id).await, None);
        assert_eq!(files_of(&dir, id), [false, false, false]);
    }
}

#[tokio::test]
#[ignore = "needs PostgreSQL: just test-chat-db"]
async fn a_settings_document_that_leaves_out_the_switches_and_the_viewer_keeps_them() {
    // The operator, 2026-11-10: a PUT without `avatar_uploads` or `evidence_viewer` (or
    // `attachments`) keeps what is stored instead of resetting it.
    use weavesignal_model::staff::Role;
    let s = server().await;
    let (viewer, _) = staff_signed_in(&s, "morven", "Morven Reid", Role::Admin).await;
    let url = format!("{}/api/v1/chat/admin/settings", s.base);
    let (_, mut doc) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    doc["avatar_uploads"] = json!(false);
    doc["attachments"] = json!(true);
    doc["evidence_viewer"] = json!(viewer.to_string());
    let (code, body) = http("PUT", url.clone(), Some(ADMIN.into()), Some(doc)).await;
    assert_eq!(code, 200, "{body}");
    let (code, body) = http(
        "PUT",
        url,
        Some(ADMIN.into()),
        Some(json!({"terms": "Be kind to each other.", "guests": true})),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let (_, now) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    assert_eq!(now["terms"], "Be kind to each other.");
    assert_eq!(now["avatar_uploads"], false, "{now}");
    assert_eq!(now["attachments"], true, "{now}");
    assert_eq!(now["evidence_viewer"], viewer.to_string(), "{now}");
    // Sent as null, it is cleared: leaving out and clearing are different.
    let mut cleared = now.clone();
    cleared["evidence_viewer"] = Value::Null;
    let (code, _) = http(
        "PUT",
        format!("{}/api/v1/chat/admin/settings", s.base),
        Some(ADMIN.into()),
        Some(cleared),
    )
    .await;
    assert_eq!(code, 200);
    let (_, after) = s.get("/api/v1/chat/admin/settings", Some(ADMIN)).await;
    assert_eq!(after["evidence_viewer"], Value::Null);
}
