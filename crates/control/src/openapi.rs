// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The public API's description (ADR-0195): an OpenAPI 3.1 document generated from the
//! handlers' `#[utoipa::path]` annotations and the request and response types' `ToSchema`
//! derives.
//!
//! Each module that serves routes has an `Api` struct listing its handlers; [`document`] merges
//! them. The router is built by hand with `.route()` and `.merge()` (three state types, per-route
//! body limits and layers), so `utoipa-axum`'s router wrapper is not used, and a test
//! (`every_route_is_described`) reads the sources' `.route(` calls and fails when a route has no
//! description, or a description has no route.
//!
//! The document is served at `GET /api/v1/openapi.json` with no bearer: the description is not
//! secret. A copy is committed at `docs/openapi/weavesignal-v1.json` and `just openapi-check`
//! compares it with this one.
//!
//! Choices recorded in ADR-0195's *In the build*: the document's `info.version` is the API's
//! (`1`), not the crate's, so a release does not change the committed file; the chat's routes are
//! not in this document (the chat is its own process, with a chatter's session as a bearer and its
//! own error body; they get a second document, made the same way, not built yet); an admin-only
//! route says so in its description and carries `x-admin: true`.

use axum::Json;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi, ToResponse, ToSchema};

/// A refusal or a failure: one plain sentence, and for a refused field its name.
#[derive(ToSchema, serde::Serialize)]
pub struct Problem {
    /// What went wrong, in a sentence for a person.
    pub error: String,
    /// The field that was refused, where the route names one (`422`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// A file as the whole body: audio to normalise, a picture.
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct File(#[allow(dead_code)] Vec<u8>);

macro_rules! problem_responses {
    ($($name:ident, $text:literal;)*) => {$(
        #[derive(ToResponse)]
        #[response(description = $text)]
        #[allow(dead_code)]
        pub struct $name(#[to_schema] Problem);
    )*};
}

problem_responses! {
    Unauthorized, "No bearer, a wrong one, or a staff session that has ended (\"Your sign-in has ended. Sign in again.\").";
    Forbidden, "The bearer is a staff session whose role may not do this: the route is admin only.";
    NotFound, "There is no such thing.";
    Conflict, "It does not fit the current state (or the desk refused it).";
    Unprocessable, "The request is well formed but not acceptable; the sentence names the problem, and `field` the field.";
    Unavailable, "The part of the station that answers is not configured or does not answer (the desk, the database, LiveKit, mail).";
}

/// `429`: a limit was reached (per address on the open routes, per API token elsewhere).
#[derive(ToResponse)]
#[response(
    description = "Too many requests just now: per address on the open routes, per API token elsewhere. `Retry-After` says how many seconds to wait.",
    headers(("Retry-After" = u64, description = "Seconds until the limit's window turns over."))
)]
#[allow(dead_code)]
pub struct TooManyRequests(#[to_schema] Problem);

/// `403` for an API token: the token does not hold the scope the route needs.
#[derive(ToResponse)]
#[response(
    description = "The bearer is an API token without the scope this route needs (\"This API token does not hold the `…` scope.\"); or a staff session whose role may not do this."
)]
#[allow(dead_code)]
pub struct ScopeMissing(#[to_schema] Problem);

/// The two bearers (ADR-0127) as security schemes, and the document's own words.
struct Bearers;

impl Modify for Bearers {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "staff_session",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "A staff session token (`rss_…`) from `POST /api/v1/sessions`. An account's role decides which routes it reaches; routes marked admin only need the `admin` role.",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "pass",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "A pass (ADR-0119): the link a guest, a remote DJ, a presenter or a caller joins by; from `POST /api/v1/invites` or `POST /api/v1/call-in`.",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "api_token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(api_token_description()))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "station_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "The station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`): the station's own credential, with an admin's powers.",
                    ))
                    .build(),
            ),
        );
    }
}

/// The token bearer's words: what a token is, and every scope with what it lets one do.
fn api_token_description() -> String {
    let mut out = String::from(
        "An API token (`rsa_…`, ADR-0195) from `POST /api/v1/tokens`, made by an admin for one integration with the scopes it needs. A token reaches only the routes its scopes open (each such route lists `api_token` with the scope it needs) and is never an admin: no route marked `x-admin` takes one. Each token is limited per minute (`429`). The scopes:\n",
    );
    for (name, what) in crate::scopes::SCOPES {
        out.push_str(&format!("\n- `{name}`: {what}"));
    }
    out
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "WeaveSignal control plane API",
        version = "1",
        description = "The station's HTTP API, `/api/v1` (ADR-0195). Within `/api/v1` changes are additive only. Bodies are JSON. The narrative (authorisation, errors, stability) is in the product documentation's *Control plane routes* page.",
        license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only"),
    ),
    modifiers(&Bearers),
    tags(
        (name = "station", description = "The station, and whether the service is alive."),
        (name = "sessions", description = "Staff sign-in: the session tokens that open the studio's routes."),
        (name = "staff", description = "Staff accounts."),
        (name = "studio", description = "The studio's view: sources, callers, chat modes, notices."),
        (name = "desk", description = "The desk's switches."),
        (name = "screener", description = "What the screener does to a caller."),
        (name = "room", description = "Room tokens and passes."),
        (name = "join", description = "Invitations and the pass a guest, a remote DJ, a presenter or a caller joins by."),
        (name = "chat", description = "The studio's chat modes. The chat service's own routes are its own document."),
        (name = "carts", description = "The cart wall."),
        (name = "shows", description = "Shows, series and episodes."),
        (name = "cards", description = "Show cards: the pictures that go with shows, series and episodes."),
        (name = "people", description = "The remembered guests and hosts."),
        (name = "schedule", description = "Clocks, the week, the log and the calendar's episodes."),
        (name = "playout", description = "The playout layer."),
        (name = "library", description = "The music library."),
        (name = "reports", description = "Plays, audience and the legal logger."),
        (name = "retention", description = "The Jurisdiction setting, holds, recordings and consents. Admin only."),
        (name = "video", description = "The slate's picture and the cameras' thumbnails."),
        (name = "delivery", description = "Content steering for the edges."),
        (name = "public", description = "Routes that need no bearer: the player, now playing, the feeds, the HLS files, and the open routes for a station's website (`/api/v1/public/…`, now playing), which carry CORS open to any origin and are limited per address."),
        (name = "tokens", description = "API tokens for integrations. Admin only."),
        (name = "webhooks", description = "Webhooks: signed deliveries of the station's events to an integration. Admin only."),
        (name = "setup", description = "The station's setup page (ADR-0237): its name and takedown contact, the stream's password where the install has one, and its look on the public pages. Admin only, except reading the look."),
        (name = "rtmp", description = "RTMP out: the destinations the desk sends the programme's picture and sound to, behind the delay (ADR-0230). Admin only; stream keys are never shown."),
    ),
    components(responses(Unauthorized, Forbidden, NotFound, Conflict, Unprocessable, Unavailable, TooManyRequests, ScopeMissing)),
    paths(openapi_json),
)]
struct Root;

/// The whole description: every module's routes in one document.
pub fn document() -> utoipa::openapi::OpenApi {
    let mut doc = Root::openapi();
    for part in [
        crate::app::Api::openapi(),
        crate::onair_api::Api::openapi(),
        crate::retention::Api::openapi(),
        crate::staff::Api::openapi(),
        crate::shows::Api::openapi(),
        crate::steps::Api::openapi(),
        crate::video::Api::openapi(),
        crate::steering::Api::openapi(),
        crate::public::Api::openapi(),
        crate::tokens::Api::openapi(),
        crate::webhooks::Api::openapi(),
        crate::open::Api::openapi(),
        crate::rtmp::Api::openapi(),
        crate::setup::Api::openapi(),
    ] {
        doc.merge(part);
    }
    tokens_where_scoped(&mut doc);
    doc
}

/// Each route a scope opens ([`crate::scopes::TABLE`]) takes an API token with that scope,
/// and may answer `403` for a token without it and `429` for a token over its limit. Written
/// here, from the table the gate obeys, so the description cannot disagree with it.
fn tokens_where_scoped(doc: &mut utoipa::openapi::OpenApi) {
    use utoipa::openapi::security::SecurityRequirement;
    use utoipa::openapi::{Ref, RefOr};
    for (method, path, scope) in crate::scopes::TABLE {
        let Some(item) = doc.paths.paths.get_mut(*path) else {
            continue;
        };
        let op = match *method {
            "GET" => item.get.as_mut(),
            "POST" => item.post.as_mut(),
            "PUT" => item.put.as_mut(),
            "DELETE" => item.delete.as_mut(),
            "PATCH" => item.patch.as_mut(),
            _ => None,
        };
        let Some(op) = op else { continue };
        op.security
            .get_or_insert_with(Vec::new)
            .push(SecurityRequirement::new("api_token", [*scope]));
        let responses = &mut op.responses.responses;
        // A token without the scope is refused with 403 too: the words say both.
        let forbidden = RefOr::Ref(Ref::from_response_name("Forbidden"));
        let missing = RefOr::Ref(Ref::from_response_name("ScopeMissing"));
        match responses.get_mut("403") {
            Some(r) if *r == forbidden => *r = missing,
            Some(_) => {}
            None => {
                responses.insert("403".into(), missing);
            }
        }
        responses
            .entry("429".into())
            .or_insert_with(|| RefOr::Ref(Ref::from_response_name("TooManyRequests")));
    }
    // The open routes are limited per address.
    for (path, item) in doc.paths.paths.iter_mut() {
        if !crate::api::is_open(path) {
            continue;
        }
        if let Some(op) = item.get.as_mut() {
            op.responses
                .responses
                .entry("429".into())
                .or_insert_with(|| RefOr::Ref(Ref::from_response_name("TooManyRequests")));
        }
    }
}

/// The description of the API, for tools and for the documentation's reference.
#[utoipa::path(
    get,
    path = "/api/v1/openapi.json",
    operation_id = "openapi_json",
    tag = "station",
    summary = "The API's description",
    description = "This document: the API as OpenAPI 3.1, generated from the handlers. Needs no bearer; the description is not secret.",
    security(()),
    responses((status = 200, description = "This document, OpenAPI 3.1.", content_type = "application/json", body = serde_json::Value)),
)]
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(document())
}

/// The document as the committed file has it: pretty JSON, a newline last.
pub fn json(doc: &utoipa::openapi::OpenApi) -> String {
    doc.to_pretty_json().expect("the description serialises")
}

/// The reference page for the product documentation, made from the description: the same
/// text for the same document, so `just openapi-check` can compare it with the committed page.
pub fn reference_page(doc: &utoipa::openapi::OpenApi) -> String {
    crate::openapi_page::render(&serde_json::to_value(doc).expect("the description serialises"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Every `(METHOD, path)` the sources register with `.route(`, tests excluded.
    fn registered() -> BTreeSet<(String, String)> {
        let dir = root().join("crates/control/src");
        let mut out = BTreeSet::new();
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "rs"))
            // This file only talks about `.route(`.
            .filter(|p| p.file_name().is_some_and(|n| n != "openapi.rs"))
            .collect();
        files.sort();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let text = text.split("\n#[cfg(test)]").next().unwrap();
            let mut rest = text;
            while let Some(at) = rest.find(".route(") {
                rest = &rest[at + ".route(".len()..];
                // The call's arguments, up to the matching bracket.
                let mut depth = 1;
                let mut end = rest.len();
                let mut in_str = false;
                for (i, c) in rest.char_indices() {
                    match c {
                        '"' => in_str = !in_str,
                        '(' if !in_str => depth += 1,
                        ')' if !in_str => {
                            depth -= 1;
                            if depth == 0 {
                                end = i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let args = &rest[..end];
                let (path, after) = if let Some(s) = args.trim_start().strip_prefix('"') {
                    let close = s.find('"').unwrap();
                    (s[..close].to_owned(), &s[close + 1..])
                } else {
                    // A constant: `STEERING_PATH`.
                    let name = args.split(',').next().unwrap().trim();
                    let decl = format!("const {name}: &str = \"");
                    let hay = std::fs::read_to_string(&file).unwrap();
                    let from = hay
                        .find(&decl)
                        .unwrap_or_else(|| panic!("no constant {name}"))
                        + decl.len();
                    let value = hay[from..].split('"').next().unwrap().to_owned();
                    (value, &args[args.find(',').unwrap()..])
                };
                // The methods at the top level of the handler chain: `get(a).put(b).layer(..)`.
                let mut depth = 0;
                let mut word = String::new();
                for c in after.chars() {
                    match c {
                        '(' => {
                            if depth == 0 {
                                let w = word.rsplit("::").next().unwrap_or("").to_owned();
                                if ["get", "post", "put", "delete", "patch"].contains(&w.as_str()) {
                                    out.insert((w.to_uppercase(), path.replace("{*", "{")));
                                }
                            }
                            depth += 1;
                            word.clear();
                        }
                        ')' => {
                            depth -= 1;
                            word.clear();
                        }
                        c if c.is_alphanumeric() || c == '_' || c == ':' => word.push(c),
                        _ => word.clear(),
                    }
                }
            }
        }
        out
    }

    fn described() -> BTreeSet<(String, String)> {
        let doc = serde_json::to_value(document()).unwrap();
        let mut out = BTreeSet::new();
        for (path, item) in doc["paths"].as_object().unwrap() {
            for method in item.as_object().unwrap().keys() {
                out.insert((method.to_uppercase(), path.clone()));
            }
        }
        out
    }

    /// A new route without its `#[utoipa::path]` (or a description left behind by a route that
    /// went) fails here, so the committed document cannot silently miss a route (ADR-0195).
    #[test]
    fn every_route_is_described() {
        let routes = registered();
        assert!(
            routes.len() > 80,
            "the scan found only {} routes",
            routes.len()
        );
        let described = described();
        let undescribed: Vec<_> = routes.difference(&described).collect();
        let stale: Vec<_> = described.difference(&routes).collect();
        assert!(
            undescribed.is_empty(),
            "routes with no #[utoipa::path] (add the handler to its module's `Api`): {undescribed:?}"
        );
        assert!(
            stale.is_empty(),
            "described but not registered with `.route(`: {stale:?}"
        );
    }

    /// Every operation says who may call it, an admin-only one can answer 403, and each has a
    /// unique id and a summary.
    #[test]
    fn every_operation_says_who_may_call_it() {
        let doc = serde_json::to_value(document()).unwrap();
        let mut ids = BTreeSet::new();
        let mut admin_routes = 0;
        for (path, item) in doc["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let here = format!("{} {path}", method.to_uppercase());
                assert!(op.get("security").is_some(), "{here} has no `security`");
                assert!(
                    op["summary"].as_str().is_some_and(|s| !s.is_empty()),
                    "{here} has no summary"
                );
                assert!(
                    ids.insert(op["operationId"].as_str().unwrap().to_owned()),
                    "{here}: the operation id is not unique"
                );
                if op.get("x-admin") == Some(&serde_json::json!(true)) {
                    admin_routes += 1;
                    assert!(
                        op["responses"].get("403").is_some(),
                        "{here} is admin only but cannot say 403"
                    );
                    assert!(
                        op["description"]
                            .as_str()
                            .unwrap()
                            .starts_with("Admin only."),
                        "{here} is admin only but does not say so"
                    );
                }
            }
        }
        assert!(
            admin_routes >= 20,
            "only {admin_routes} admin-only routes carry `x-admin`: the extension is being dropped"
        );
    }

    /// Every route registered anywhere is placed for tokens: a scope in `scopes::TABLE`, or
    /// `scopes::NEVER` (the scopes' own test reads a fixed list of files; this reads them all).
    #[test]
    fn every_route_is_placed_for_tokens() {
        let unplaced: Vec<_> = registered()
            .into_iter()
            .filter(|(m, p)| {
                !crate::scopes::TABLE
                    .iter()
                    .any(|(tm, tp, _)| tm == m && tp == p)
                    && !crate::scopes::NEVER.contains(&p.as_str())
            })
            .collect();
        assert!(
            unplaced.is_empty(),
            "decide for each: a scope in scopes::TABLE, or scopes::NEVER: {unplaced:?}"
        );
    }

    /// Every route a scope opens is in the description and says so with its scope; no admin
    /// route takes a token.
    #[test]
    fn every_scoped_route_takes_its_token() {
        let doc = serde_json::to_value(document()).unwrap();
        for (method, path, scope) in crate::scopes::TABLE {
            let op = &doc["paths"][*path][method.to_lowercase()];
            assert!(
                op.is_object(),
                "{method} {path} is in scopes::TABLE but not described"
            );
            let security = op["security"].as_array().unwrap();
            assert!(
                security
                    .iter()
                    .any(|s| s["api_token"] == serde_json::json!([scope])),
                "{method} {path} does not list api_token with {scope}"
            );
            assert!(
                op["responses"].get("429").is_some(),
                "{method} {path}: no 429"
            );
        }
        for (path, item) in doc["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let takes_token = op["security"]
                    .as_array()
                    .is_some_and(|s| s.iter().any(|s| s.get("api_token").is_some()));
                assert!(
                    !(takes_token && op.get("x-admin") == Some(&serde_json::json!(true))),
                    "{method} {path} is admin only but lists api_token"
                );
            }
        }
    }

    /// The committed copy is the generated one. `just openapi` writes it.
    #[test]
    fn the_committed_description_is_current() {
        let doc = document();
        for (name, want) in [
            (
                "docs/openapi/weavesignal-v1.json",
                format!("{}\n", json(&doc)),
            ),
            (
                "docs/src/content/docs/docs/reference/api.md",
                reference_page(&doc),
            ),
        ] {
            let have = std::fs::read_to_string(root().join(name)).unwrap_or_default();
            assert!(
                have == want,
                "{name} is out of date: run `just openapi` and commit the result"
            );
        }
    }
}
