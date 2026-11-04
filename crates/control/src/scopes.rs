// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The scopes an API token can hold, and which route each opens (ADR-0195).
//!
//! A token passes a route only if the route is in [`TABLE`] and the token holds the scope the
//! table names. A route that is not in the table is **not reachable with a token**, whatever
//! helper its handler calls; so a new route is closed to tokens until someone decides. Never in
//! the table, on purpose ([`NEVER`]): staff accounts and sessions, retention, holds and
//! erasure, people, station settings (`/api/v1/playout/settings`), the room's tokens (they
//! mint presenters), invites, API tokens and webhooks. A token is never an admin.
//!
//! The scope covers one resource and one kind of use. `:write` does not imply `:read`.

use axum::http::Method;

/// Every scope, with what it lets an integration do.
pub const SCOPES: &[(&str, &str)] = &[
    (
        "studio:read",
        "Read the studio's view: sources, callers (with the names and topics they gave), chat modes, notices; and follow it live.",
    ),
    (
        "desk:control",
        "Work the desk: on air, mute, cue, talk, and dump the profanity delay.",
    ),
    (
        "screener:control",
        "Work the screener: screen, send, decline and drop callers; end a pass.",
    ),
    (
        "chat:moderate",
        "Change the chat's modes (slow mode, hold new, links). The chat's own moderation routes are not reachable with a token.",
    ),
    ("carts:read", "Read the cart wall's banks and carts."),
    (
        "carts:fire",
        "Fire, stop and cue carts; stop the last; fade all.",
    ),
    (
        "carts:write",
        "Make and rename banks; load and empty cart slots.",
    ),
    (
        "schedule:read",
        "Read the clocks, the week, the log of what is to play, and the episodes airing in a range.",
    ),
    (
        "schedule:write",
        "Change clocks, the week and the log; generate a day.",
    ),
    (
        "playout:read",
        "Read playout's status and zone, the logger, and the track library.",
    ),
    (
        "library:write",
        "Add tracks to the library and delete them.",
    ),
    ("reports:read", "Read the play and audience reports."),
    (
        "shows:read",
        "Read shows, series, episodes and corrections.",
    ),
    (
        "shows:write",
        "Change shows, series and episodes; put an episode on air; correct one; publish its files; draw cards.",
    ),
    (
        "video:read",
        "Read the video slate's still and the cameras' thumbnails.",
    ),
    ("video:write", "Change or remove the video slate's still."),
];

/// Whether `name` is a scope.
pub fn is_scope(name: &str) -> bool {
    SCOPES.iter().any(|(n, _)| *n == name)
}

/// (method, route as registered, scope).
pub const TABLE: &[(&str, &str, &str)] = &[
    ("GET", "/api/v1/studio", "studio:read"),
    ("GET", "/api/v1/studio/events", "studio:read"),
    ("POST", "/api/v1/desk/commands", "desk:control"),
    ("POST", "/api/v1/screener/commands", "screener:control"),
    ("POST", "/api/v1/passes/end", "screener:control"),
    ("POST", "/api/v1/chat/modes", "chat:moderate"),
    ("GET", "/api/v1/carts", "carts:read"),
    ("POST", "/api/v1/carts/commands", "carts:fire"),
    ("PUT", "/api/v1/carts/banks/{number}", "carts:write"),
    (
        "PUT",
        "/api/v1/carts/banks/{bank}/slots/{slot}",
        "carts:write",
    ),
    (
        "DELETE",
        "/api/v1/carts/banks/{bank}/slots/{slot}",
        "carts:write",
    ),
    ("GET", "/api/v1/video/still", "video:read"),
    ("GET", "/api/v1/video/still/image", "video:read"),
    ("GET", "/api/v1/video/thumbs", "video:read"),
    ("PUT", "/api/v1/video/still", "video:write"),
    ("DELETE", "/api/v1/video/still", "video:write"),
    ("GET", "/api/v1/shows/{show}/still", "video:read"),
    ("GET", "/api/v1/shows/{show}/still/image", "video:read"),
    ("PUT", "/api/v1/shows/{show}/still", "video:write"),
    ("DELETE", "/api/v1/shows/{show}/still", "video:write"),
    ("GET", "/api/v1/playout", "playout:read"),
    ("GET", "/api/v1/playout/zone", "playout:read"),
    ("GET", "/api/v1/logger", "playout:read"),
    ("GET", "/api/v1/library/tracks", "playout:read"),
    ("PUT", "/api/v1/library/tracks", "library:write"),
    ("DELETE", "/api/v1/library/tracks/{id}", "library:write"),
    ("GET", "/api/v1/schedule/clocks", "schedule:read"),
    ("PUT", "/api/v1/schedule/clocks/{id}", "schedule:write"),
    ("DELETE", "/api/v1/schedule/clocks/{id}", "schedule:write"),
    ("GET", "/api/v1/schedule/week", "schedule:read"),
    ("PUT", "/api/v1/schedule/week", "schedule:write"),
    ("POST", "/api/v1/schedule/generate", "schedule:write"),
    ("GET", "/api/v1/schedule/log", "schedule:read"),
    ("PUT", "/api/v1/schedule/log/{id}", "schedule:write"),
    ("DELETE", "/api/v1/schedule/log/{id}", "schedule:write"),
    ("GET", "/api/v1/schedule/episodes", "schedule:read"),
    ("GET", "/api/v1/reports/plays", "reports:read"),
    ("GET", "/api/v1/reports/audience", "reports:read"),
    ("GET", "/api/v1/shows", "shows:read"),
    ("GET", "/api/v1/shows/{show}", "shows:read"),
    ("PUT", "/api/v1/shows/{show}", "shows:write"),
    ("POST", "/api/v1/shows/{show}/cards", "shows:write"),
    ("GET", "/api/v1/shows/{show}/series/{series}", "shows:read"),
    ("PUT", "/api/v1/shows/{show}/series/{series}", "shows:write"),
    (
        "POST",
        "/api/v1/shows/{show}/series/{series}/cards",
        "shows:write",
    ),
    (
        "POST",
        "/api/v1/shows/{show}/series/{series}/episodes",
        "shows:write",
    ),
    (
        "PATCH",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}",
        "shows:write",
    ),
    (
        "POST",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}/cards",
        "shows:write",
    ),
    (
        "POST",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}/air",
        "shows:write",
    ),
    (
        "PUT",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}/files",
        "shows:write",
    ),
    (
        "POST",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}/corrections",
        "shows:write",
    ),
    (
        "GET",
        "/api/v1/shows/{show}/series/{series}/episodes/{episode}/corrections",
        "shows:read",
    ),
    ("POST", "/api/v1/cards/preview", "shows:write"),
    ("POST", "/api/v1/cards/words", "shows:write"),
];

/// Routes a token never reaches, named so that a new route has to be placed in one list or
/// the other (a test reads the route registrations and checks every route is in a list).
pub const NEVER: &[&str] = &[
    "/health",
    "/api/v1/openapi.json",
    // Files every listener reaches without a bearer.
    "/feeds/{file}",
    "/hls/{path}",
    "/api/v1/station",
    "/api/v1/room/tokens",
    "/api/v1/invites",
    "/api/v1/player",
    "/api/v1/call-in",
    "/api/v1/call-in/challenge",
    "/api/v1/pass",
    "/api/v1/pass/token",
    "/api/v1/pass/events",
    "/api/v1/pass/leave",
    "/api/v1/playout/settings",
    "/api/v1/people",
    "/api/v1/people/{id}/erase",
    "/api/v1/now-playing",
    "/api/v1/now-playing/events",
    "/api/v1/sessions",
    "/api/v1/sessions/code",
    "/api/v1/sessions/code/finish",
    "/api/v1/sessions/me",
    "/api/v1/sessions/me/email",
    "/api/v1/sessions/me/password",
    "/api/v1/staff",
    "/api/v1/staff/{id}",
    "/api/v1/staff/{id}/password",
    "/api/v1/staff/{id}/email",
    "/api/v1/retention",
    "/api/v1/retention/preview",
    "/api/v1/retention/sweep",
    "/api/v1/retention/holds",
    "/api/v1/retention/holds/{id}/release",
    "/api/v1/retention/holds/{id}/extend",
    "/api/v1/retention/recordings",
    "/api/v1/retention/recordings/{id}/end",
    "/api/v1/retention/consents/{id}/recordings",
    "/api/v1/retention/consents",
    "/api/v1/retention/consents/{id}/erase",
    "/api/v1/steering",
    "/api/v1/steering/edges",
    "/api/v1/tokens",
    "/api/v1/tokens/{id}",
    "/api/v1/webhooks",
    "/api/v1/webhooks/{id}",
    "/api/v1/webhooks/{id}/secret",
    "/api/v1/public/station",
    "/api/v1/public/schedule",
    "/api/v1/public/episodes",
];

/// The scope a route needs, for a method and the route as registered (`MatchedPath`).
pub fn scope_for(method: &Method, route: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(m, r, _)| *m == method.as_str() && *r == route)
        .map(|(_, _, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `"/api/v1/..."` string a source file registers outside its tests and comments.
    fn registered(src: &str) -> Vec<String> {
        let live = src.split("#[cfg(test)]").next().unwrap_or(src);
        let mut out = Vec::new();
        for line in live.lines() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            let mut rest = t;
            while let Some(i) = rest.find("\"/") {
                let tail = &rest[i + 1..];
                let Some(end) = tail.find('"') else { break };
                let lit = &tail[..end];
                if lit.starts_with("/api/v1/") || lit == "/health" {
                    out.push(lit.to_owned());
                }
                rest = &tail[end + 1..];
            }
        }
        out
    }

    #[test]
    fn every_registered_route_is_either_scoped_or_never() {
        let sources = [
            ("app.rs", include_str!("app.rs")),
            ("onair_api.rs", include_str!("onair_api.rs")),
            ("shows.rs", include_str!("shows.rs")),
            ("video.rs", include_str!("video.rs")),
            ("retention.rs", include_str!("retention.rs")),
            ("staff.rs", include_str!("staff.rs")),
            ("steering.rs", include_str!("steering.rs")),
            ("tokens.rs", include_str!("tokens.rs")),
            ("webhooks.rs", include_str!("webhooks.rs")),
            ("open.rs", include_str!("open.rs")),
        ];
        let mut unplaced = Vec::new();
        let mut seen = 0;
        for (file, src) in sources {
            for lit in registered(src) {
                seen += 1;
                let scoped = TABLE.iter().any(|(_, r, _)| *r == lit);
                if !scoped && !NEVER.contains(&lit.as_str()) {
                    unplaced.push(format!("{file}: {lit}"));
                }
            }
        }
        assert!(seen > 80, "the scan found only {seen} routes");
        assert!(
            unplaced.is_empty(),
            "decide for each: a scope in TABLE, or NEVER\n{}",
            unplaced.join("\n")
        );
    }

    #[test]
    fn nothing_is_both_scoped_and_never() {
        for (_, r, _) in TABLE {
            assert!(!NEVER.contains(r), "{r} is in both lists");
        }
    }

    #[test]
    fn every_scope_in_the_table_is_defined_and_used() {
        for (_, r, s) in TABLE {
            assert!(is_scope(s), "{r} names the unknown scope {s}");
        }
        for (s, _) in SCOPES {
            assert!(
                TABLE.iter().any(|(_, _, t)| t == s),
                "scope {s} opens nothing"
            );
        }
        // No scope is named like an admin power.
        assert!(SCOPES.iter().all(|(n, _)| !n.contains("admin")));
    }

    #[test]
    fn a_route_is_found_by_method_and_pattern_only() {
        assert_eq!(
            scope_for(&Method::POST, "/api/v1/carts/commands"),
            Some("carts:fire")
        );
        assert_eq!(scope_for(&Method::GET, "/api/v1/carts/commands"), None);
        assert_eq!(scope_for(&Method::GET, "/api/v1/staff"), None);
        assert_eq!(scope_for(&Method::GET, "/api/v1/tokens"), None);
        assert_eq!(scope_for(&Method::PUT, "/api/v1/playout/settings"), None);
    }
}
