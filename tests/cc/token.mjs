// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A LiveKit token as the control plane would sign it (crates/proto/src/token.rs: HS256, the role
// as the `weavesignal.role` attribute), for tests that run the desk without a control plane.
//
//   node token.mjs KEY SECRET ROOM IDENTITY ROLE [subscribe]     -> prints the token
import { createHmac } from 'node:crypto';

const [key, secret, room, identity, role, subscribe] = process.argv.slice(2);
const b64 = (o) => Buffer.from(typeof o === 'string' ? o : JSON.stringify(o)).toString('base64url');
const now = Math.floor(Date.now() / 1000);
const claims = {
  iss: key,
  sub: identity,
  nbf: now - 10,
  exp: now + 3600,
  name: identity,
  video: {
    room,
    roomJoin: true,
    canPublish: role !== 'listener',
    canSubscribe: subscribe === 'subscribe',
    canPublishData: false,
  },
  attributes: { 'weavesignal.role': role },
};
const input = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64(claims)}`;
console.log(`${input}.${createHmac('sha256', secret).update(input).digest('base64url')}`);
