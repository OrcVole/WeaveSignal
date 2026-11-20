// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What WeaveSignal's processes share: the roles people take in the room, the rules for their
//! identities, the LiveKit access tokens the control plane issues and the desk reads, the
//! station's optional STUN servers, and the guard on a pulled source's address.

pub mod booth;
pub mod bridge_wire;
pub mod forwarded;
pub mod monitor;
pub mod pass;
pub mod phone;
pub mod pull;
pub mod role;
pub mod source_wire;
pub mod stun;
pub mod token;
pub mod vban;

pub use booth::{booth, booth_owner, booth_prefix, room_for};
pub use forwarded::last_forwarded;
pub use monitor::{
    is_monitor_seat, monitor_key, monitor_prefix, monitor_room, monitor_seat, monitor_seats,
};
pub use pass::{MAX_PASS_TTL_S, PASS_PREFIX, PassClaims, PassKind};
pub use role::{IdentityError, ROLE_ATTRIBUTE, Role, check_identity};
pub use stun::{MAX_STUN_SERVERS, parse_stun_servers, webrtcbin_url};
pub use token::{Grants, Keys, TokenClaims, TokenError};
