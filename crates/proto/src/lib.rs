// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What WeaveSignal's processes share: the roles people take in the room, the rules for their
//! identities, and the LiveKit access tokens the control plane issues and the desk reads.

pub mod booth;
pub mod forwarded;
pub mod pass;
pub mod role;
pub mod token;

pub use booth::{booth, booth_owner, booth_prefix, room_for};
pub use forwarded::last_forwarded;
pub use pass::{MAX_PASS_TTL_S, PASS_PREFIX, PassClaims, PassKind};
pub use role::{IdentityError, ROLE_ATTRIBUTE, Role, check_identity};
pub use token::{Grants, Keys, TokenClaims, TokenError};
