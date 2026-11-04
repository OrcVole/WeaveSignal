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
}

async fn server() -> Server {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let station = uuid::Uuid::new_v4();
    let outbox = std::env::temp_dir().join(format!("weavesignal-chat-test-{station}"));
    let mut c = Config::from_toml(
        &SAMPLE.replace("00000000-0000-0000-0000-000000000000", &station.to_string()),
        "test",
    )
    .unwrap();
    c.control.url = String::new();
    c.mail.mode = MailMode::Outbox;
    c.mail.outbox_dir = outbox.clone();
    c.database_url = Some(url.clone());
    c.admin_token = Some(ADMIN.into());
    c.email_key = Some(EmailKey::parse(KEY).unwrap());
    let state = weavesignal_chat::start(c).await.unwrap();
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
        async move {
            sqlx::query(
                "UPDATE chat_message SET server_at = now() - make_interval(secs => $2) WHERE id = $1",
            )
            .bind(id)
            .bind(hours * 3600.0)
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
