// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Keyed shows at the edge (1.3.2 spike, PLAN 19a): the token check, in nginx's njs.
//
// A keyed request carries its token in the path: /hls/k/<show>/<token>/<file> and
// /listen/k/<show>/<token>/<mount>. `check` runs as the auth_request of every such request
// (playlists and segments, HEAD and Range, cache hits included: auth runs before the cache), and
// answers 204 or 403. It needs the station's ECDSA P-256 public key ($keyed_public_key) and the
// clock; no secret of the station's, no call home. A token is `ks1.<claims>.<signature>`
// (crates/keyed in the repository makes and checks the same).
//
// `cut` is the body filter of keyed listener streams: a stream outlives the token it began with,
// so it is ended when the token expires or its key is revoked (the player renews and reconnects).
//
// `refresh` (js_periodic) keeps the revoked keys' list ($keyed_revoked_url, JSON
// {"revoked": ["<key id>", ...]}) in a shared dictionary, so a revoke reaches the edge within one
// interval whatever the tokens' lives.

var keyPromise = null;

function publicKey(r) {
  if (!keyPromise) {
    keyPromise = crypto.subtle.importKey(
      'raw', Buffer.from(r.variables.keyed_public_key, 'base64url'),
      { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
  }
  return keyPromise;
}

// The auth subrequest sees the request line as sent ($request_uri), not the path nginx
// normalised and fetches. They are the same when the line has no percent sign, no segment that
// starts with a dot and no empty segment, so anything else is refused: otherwise
// `/hls/k/A/<token for A>/../../B/<token>/x` would be judged as A and fetch B.
var PATH = /^\/(hls|listen)\/k\/([A-Za-z0-9_-]{1,64})\/(ks1\.[A-Za-z0-9_-]{1,400}\.[A-Za-z0-9_-]{86})\/([A-Za-z0-9_][A-Za-z0-9_.\/-]{0,200})$/;

function parse(uri) {
  var m = PATH.exec(uri);
  if (!m) return null;
  var media = m[1] === 'listen' ? 'a'
    : (m[4] === 'audio.m3u8' || m[4].indexOf('audio/') === 0) ? 'a' : 'v';
  return { kind: m[1], show: m[2], token: m[3], rest: m[4], media: media };
}

function claimsOf(token) {
  try {
    return JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString());
  } catch (e) {
    return null;
  }
}

function revoked(key) {
  var d = ngx.shared.keyed_revoked;
  return !!(d && d.get(key));
}

async function verify(r, p) {
  var parts = p.token.split('.');
  var sig = Buffer.from(parts[2], 'base64url');
  if (parts[0] !== 'ks1' || sig.length !== 64) return 'malformed';
  var ok = await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, await publicKey(r), sig,
    Buffer.from(parts[0] + '.' + parts[1]));
  if (!ok) return 'bad-signature';
  var c = claimsOf(p.token);
  if (!c || typeof c.nbf !== 'number' || typeof c.exp !== 'number') return 'malformed';
  var now = Date.now() / 1000;
  var skew = Number(r.variables.keyed_skew || 5);
  if (now + skew < c.nbf) return 'not-yet';
  if (now - skew >= c.exp) return 'expired';
  if (c.s !== p.show) return 'wrong-show';
  if (!(c.m === 'av' || c.m === p.media)) return 'wrong-media';
  if (revoked(c.k)) return 'revoked';
  return 'ok';
}

async function check(r) {
  var raw = (r.variables.request_uri || '').split('?')[0];
  var p = parse(raw);
  var why;
  if (!p || raw.indexOf('%') >= 0 || raw.indexOf('/.') >= 0 || raw.indexOf('//') >= 0) {
    why = 'malformed';
  } else {
    try {
      why = await verify(r, p);
    } catch (e) {
      why = 'error';
      r.error('keyed: ' + e);
    }
  }
  if (why !== 'ok') r.log('keyed: refused (' + why + ') ' + raw.slice(0, 40));
  r.return(why === 'ok' ? 204 : 403);
}

function cut(r, data, flags) {
  if (r.status !== 200) {
    r.sendBuffer(data, flags);
    return;
  }
  if (r.keyedEnd === undefined) {
    // The request line, not r.uri: the location rewrites the path to the mount's before this.
    var p = parse((r.variables.request_uri || '').split('?')[0]);
    var c = p && claimsOf(p.token);
    r.keyedEnd = c ? c.exp + Number(r.variables.keyed_skew || 5) : 0;
    r.keyedKey = c ? c.k : '';
    r.keyedDone = false;
  }
  if (r.keyedDone) return;
  if (Date.now() / 1000 >= r.keyedEnd || revoked(r.keyedKey)) {
    r.keyedDone = true;
    r.sendBuffer('', { last: true });
    return;
  }
  r.sendBuffer(data, flags);
}

async function refresh(s) {
  var url = s.variables.keyed_revoked_url;
  if (!url) return;
  try {
    var reply = await ngx.fetch(url, { max_response_body_size: 1048576 });
    if (reply.status !== 200) return;
    var list = (await reply.json()).revoked || [];
    var d = ngx.shared.keyed_revoked;
    var keep = {};
    for (var i = 0; i < list.length; i++) {
      keep[list[i]] = true;
      d.set(String(list[i]), '1');
    }
    var have = d.keys();
    for (var j = 0; j < have.length; j++) {
      if (!keep[have[j]]) d.delete(have[j]);
    }
  } catch (e) {
    ngx.log(ngx.WARN, 'keyed: the revoked list could not be read: ' + e);
  }
}

export default { check, cut, refresh };
