// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Staff accounts in the studio app (ADR-0127) without a station: the rules a form checks
// before asking, a refusal placed under its field, who may manage what, and the sign-in and
// Staff screens rendered on the server. Sample people come from the Scottish names list
// (ADR-0071), as the control plane's own tests name them.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { PANELS, SETTINGS } from '../../../packages/ui/panels.ts';
import { FieldError, type Me, type StaffAccount } from './lib/api.ts';
import {
  cleanCode,
  codeProblem,
  emailProblem,
  isAdmin,
  nameProblem,
  passwordProblem,
  placeError,
  roleWord,
  selfLockout,
  suggestPassword,
  usernameProblem,
  whoLine,
} from './lib/account.ts';
import Settings from './settings/Settings.svelte';
import Staff from './settings/Staff.svelte';
import StationSetup from './lib/StationSetup.svelte';

const morag: StaffAccount = { id: 'a1', username: 'morag', name: 'Morag Campbell', role: 'admin', disabled_ms: null, created_ms: 1 };
const fraser: StaffAccount = { id: 'a2', username: 'fraser', name: 'Fraser Macleod', role: 'presenter', disabled_ms: null, created_ms: 2 };
const ailsa: StaffAccount = { id: 'a3', username: 'ailsa', name: 'Ailsa', role: 'screener', disabled_ms: Date.UTC(2026, 10, 1), created_ms: 3 };
const elspeth: StaffAccount = { id: 'a4', username: 'elspeth.g', name: 'Elspeth Gordon', role: 'producer', disabled_ms: null, created_ms: 4 };

const asMorag: Me = { kind: 'staff', role: 'admin', account: morag, expires_ms: 9 };
const asFraser: Me = { kind: 'staff', role: 'presenter', account: fraser, expires_ms: 9 };
const asKey: Me = { kind: 'station', role: 'admin' };

describe('who may manage what', () => {
  it('an admin or the station key manages staff, retention and erasure; no other role does', () => {
    expect(isAdmin(asMorag)).toBe(true);
    expect(isAdmin(asKey)).toBe(true);
    expect(isAdmin(asFraser)).toBe(false);
    expect(isAdmin(null)).toBe(false);
  });

  it('names the signed-in person and their role, or the station key', () => {
    expect(whoLine(asMorag)).toEqual({ name: 'Morag Campbell', role: 'admin' });
    expect(whoLine(asFraser)).toEqual({ name: 'Fraser Macleod', role: 'presenter' });
    expect(whoLine(asKey)).toEqual({ name: 'Station key', role: 'admin' });
    expect(whoLine(null)).toEqual({ name: 'Station key', role: 'admin' });
    expect(roleWord('screener')).toBe('Screener');
  });

  it('an admin cannot turn off or demote their own account, and the form knows before asking', () => {
    expect(selfLockout(asMorag, morag.id, { disabled: true })).toBe(true);
    expect(selfLockout(asMorag, morag.id, { role: 'producer' })).toBe(true);
    expect(selfLockout(asMorag, morag.id, { role: 'admin' })).toBe(false);
    expect(selfLockout(asMorag, fraser.id, { disabled: true })).toBe(false);
    // The station key is nobody's account.
    expect(selfLockout(asKey, morag.id, { disabled: true })).toBe(false);
  });
});

describe('the rules a form checks before asking the station', () => {
  it('a username is lower-case letters, digits, dots, underscores and hyphens, up to 40', () => {
    expect(usernameProblem('morag')).toBeNull();
    expect(usernameProblem('elspeth.g')).toBeNull();
    expect(usernameProblem('a_b-c9')).toBeNull();
    expect(usernameProblem('')).toBe('Enter a username.');
    expect(usernameProblem('Morag')).toMatch(/lower-case/);
    expect(usernameProblem('.morag')).toMatch(/starts with a letter or digit/);
    expect(usernameProblem('a'.repeat(41))).toBe('A username is up to 40 characters.');
  });

  it('a password is 10 to 256 characters and not the username; nothing else', () => {
    expect(passwordProblem('a long enough phrase', 'morag')).toBeNull();
    expect(passwordProblem('')).toBe('Enter a password.');
    expect(passwordProblem('short one')).toMatch(/at least 10 characters/);
    expect(passwordProblem('x'.repeat(257))).toBe('A password is up to 256 characters.');
    expect(passwordProblem('elspeth.g.', 'elspeth.g.')).toBe('The password cannot be the username.');
    // Any characters count, by code point.
    expect(passwordProblem('ùisge beatha')).toBeNull();
  });

  it('a name is 1 to 80 characters', () => {
    expect(nameProblem('Fraser Macleod')).toBeNull();
    expect(nameProblem('  ')).toMatch(/Enter their name/);
    expect(nameProblem('x'.repeat(81))).toBe('A name is up to 80 characters.');
  });

  it('suggests a password from an alphabet with no look-alikes, four groups of four', () => {
    let n = 0;
    const p = suggestPassword(() => (n++ % 31) / 31);
    expect(p).toMatch(/^[a-hj-km-np-z2-9]{4}(-[a-hj-km-np-z2-9]{4}){3}$/);
    expect(passwordProblem(p)).toBeNull();
    expect(suggestPassword()).not.toBe(suggestPassword());
  });
});

describe('a refusal lands under its field', () => {
  it('places the station’s sentence under the field it names, or on the form', () => {
    expect(placeError(new FieldError(422, 'That username is taken.', 'username'), ['username', 'name'])).toEqual({
      field: 'username',
      message: 'That username is taken.',
    });
    // A field the form does not have goes to the form.
    expect(placeError(new FieldError(422, 'A name is 1 to 80 characters.', 'name'), ['password'])).toEqual({
      field: null,
      message: 'A name is 1 to 80 characters.',
    });
    expect(placeError(new FieldError(422, 'You cannot turn off your own account or take away your own admin role. Another admin can.', null), ['name']).field).toBeNull();
    // Rust refuses with a plain string; anything else gets a plain sentence.
    expect(placeError('No station answered at that address.', [])).toEqual({ field: null, message: 'No station answered at that address.' });
    expect(placeError(new Error('boom'), []).message).toBe('boom');
    expect(placeError(undefined, []).message).toBe('Something went wrong. Try again.');
  });
});

describe('the sign-in screen', () => {
  it('asks for the station, a username and a password, with the station key as the quieter route', () => {
    const { body } = render(StationSetup, { props: { initialUrl: 'https://radio.example.org' } });
    expect(body).toContain('data-panel="setup.sign-in"');
    expect(body).toMatch(/<label for="si-url"[^>]*>Station address<\/label>/);
    expect(body).toMatch(/<label for="si-username"[^>]*>Username<\/label>/);
    expect(body).toMatch(/<label for="si-password"[^>]*>Password<\/label>/);
    expect(body).toMatch(/id="si-username"[^>]*autocomplete="username"/);
    expect(body).toMatch(/id="si-password"[^>]*autocomplete="current-password"/);
    expect(body).toContain('value="https://radio.example.org"');
    expect(body).toContain('>Sign in</button>');
    expect(body).toContain('Use the station key instead');
    expect(body).not.toContain('Admin token');
    expect(PANELS['setup.sign-in']).toBeTruthy();
  });
});

describe('the emailed code (ADR-0127)', () => {
  const clean = (body: string) => body.replace(/<!--[^>]*-->/g, '');

  it('the password screen offers a code and the station key as the other ways in', () => {
    const { body } = render(StationSetup, { props: { initialUrl: 'https://radio.example.org' } });
    expect(body).toContain('Email me a code instead');
    expect(body).toContain('Use the station key instead');
    expect(body).not.toContain('id="si-email"');
    expect(body).not.toContain('id="si-code"');
  });

  it('the first step takes the station, the username and the address, and asks for a code', () => {
    const { body } = render(StationSetup, { props: { initialUrl: 'https://radio.example.org', initialMode: 'code' } });
    expect(body).toContain('data-panel="setup.sign-in"');
    expect(body).toMatch(/<label for="si-url"[^>]*>Station address<\/label>/);
    expect(body).toMatch(/<label for="si-username"[^>]*>Username<\/label>/);
    expect(body).toMatch(/<label for="si-email"[^>]*>Email address<\/label>/);
    expect(body).toMatch(/id="si-email"[^>]*type="email"|type="email"[^>]*id="si-email"/);
    expect(body).toMatch(/autocomplete="email"/);
    expect(body).toContain('>Email me a code</button>');
    expect(body).toContain('Sign in with a password instead');
    expect(body).not.toContain('id="si-password"');
    expect(body).not.toContain('id="si-code"');
  });

  it('the second step takes the six digits, says nothing of whether an account matched, and offers a way back', () => {
    const { body } = render(StationSetup, {
      props: { initialUrl: 'https://radio.example.org', initialMode: 'code', initialAsked: true },
    });
    expect(body).toMatch(/<label for="si-code"[^>]*>Code from the email<\/label>/);
    expect(body).toMatch(/autocomplete="one-time-code"/);
    expect(body).toMatch(/inputmode="numeric"/);
    // The same words whether or not the address matched an account.
    expect(clean(body)).toContain('If that username and address belong to an account here, a code is on its way.');
    expect(body).toContain('>Sign in</button>');
    expect(body).toContain('Send another code');
    expect(body).toContain('Use a different address');
    expect(body).not.toContain('id="si-email"');
  });

  it('checks the shape of an address and of a code before asking the station', () => {
    expect(emailProblem('morag.campbell@mail.example')).toBeNull();
    expect(emailProblem('  Fraser@Mail.Example ')).toBeNull();
    expect(emailProblem('')).toBe('Enter your email address.');
    for (const bad of ['morag', 'morag@', '@mail.example', 'morag@mail', 'morag@.example', 'morag@mail.', 'mo rag@mail.example', 'a@b.example, c@d.example', '<a@b.example>']) {
      expect(emailProblem(bad), bad).toBe('Enter an email address, like name@example.com.');
    }
    expect(codeProblem('123456')).toBeNull();
    expect(codeProblem('123 456')).toBeNull();
    expect(cleanCode(' 123 456\n')).toBe('123456');
    expect(codeProblem('')).toBe('Enter the code from the email.');
    expect(codeProblem('12345')).toBe('The code is six digits.');
    expect(codeProblem('12345a')).toBe('The code is six digits.');
    expect(codeProblem('1234567')).toBe('The code is six digits.');
  });

  it('a refusal for the address lands under its field, and any other on the form', () => {
    expect(placeError(new FieldError(422, 'Enter an email address.', 'email'), ['email'] as const)).toEqual({
      field: 'email',
      message: 'Enter an email address.',
    });
    expect(placeError('This station does not email sign-in codes. Sign in with your password.', ['email'] as const).field).toBeNull();
  });

  it('Settings > Staff shows who has code sign-in and offers an admin the address to set', () => {
    const withAddress: StaffAccount = { ...fraser, has_email: true };
    const { body } = render(Staff, { props: { me: asMorag, initial: [morag, withAddress] } });
    const text = clean(body);
    expect(text.match(/Code sign-in on/g)?.length).toBe(1);
    expect(body).toContain('Email…<span class="sr-only"> for Fraser Macleod’s sign-in codes</span>');
    expect(body).toContain('Email…<span class="sr-only"> for Morag Campbell’s sign-in codes</span>');
    // The dialog never shows an address back: it asks for one, and can take it away.
    expect(body).toMatch(/<label for="se-email"[^>]*>Email address<\/label>/);
    expect(text).toContain('never shows it back');
    expect(text).not.toMatch(/@mail\.example/);
  });
});

describe('Settings > Staff', () => {
  it('lists each person with their username, role and whether the account is on, you marked', () => {
    const { body } = render(Staff, { props: { me: asMorag, initial: [fraser, ailsa, morag, elspeth] } });
    expect(body).toContain('data-panel="settings.staff"');
    expect(PANELS['settings.staff']).toBeTruthy();
    for (const a of [morag, fraser, ailsa, elspeth]) {
      expect(body).toContain(a.name);
      expect(body).toContain(`>${a.username}</td>`);
      expect(body).toContain(`for="role-${a.id}">Role of ${a.name}</label>`);
    }
    expect(body).toMatch(/Morag Campbell<!--\[0--><span class="you[^"]*"> \(you\)<\/span>/);
    expect(body.replace(/<!--[^>]*-->/g, '')).toContain('3 people can sign in, 1 off');
    expect(body).toContain('Off</span>');
    expect(body).toContain('since 1 Nov');
    // The people who can sign in come first, by name; the account that is off last.
    const order = [morag, fraser, ailsa, elspeth].map((a) => body.indexOf(`>${a.username}</td>`));
    expect(order[3]).toBeLessThan(order[1]);
    expect(order[1]).toBeLessThan(order[0]);
    expect(order[0]).toBeLessThan(order[2]);
    expect(body).toContain('Add someone…');
    expect(body).toContain('Turn on<span class="sr-only"> Ailsa’s account</span>');
    expect(body).toContain('Turn off…<span class="sr-only"> Fraser Macleod’s account</span>');
    expect(body).toContain('Set password…<span class="sr-only"> for Elspeth Gordon</span>');
  });

  it('will not let an admin turn off or demote themselves, and says who can', () => {
    const { body } = render(Staff, { props: { me: asMorag, initial: [morag, fraser] } });
    expect(body).toMatch(/id="role-a1"[^>]*disabled/);
    expect(body).toContain('Another admin changes your role or turns your account off.');
    expect(body).toMatch(/class="btn sm quiet[^"]*" disabled=""/);
    expect(body).not.toMatch(/id="role-a2"[^>]*disabled/);
  });

  it('says so when no one has an account yet', () => {
    const { body } = render(Staff, { props: { me: asKey, initial: [] } });
    expect(body).toContain('No one has an account yet.');
  });

  it('the add form asks for a name, a username, a role and a first password the admin passes on', () => {
    const { body } = render(Staff, { props: { me: asKey, initial: [] } });
    for (const f of ['name', 'username', 'role', 'password']) expect(body).toMatch(new RegExp(`<label for="ad-${f}"[^>]*>`));
    expect(body).toMatch(/id="ad-password"[^>]*type="text"/);
    expect(body).toMatch(/id="ad-password"[^>]*autocomplete="off"/);
    expect(body).toContain('Pass it on to them; they can change it once signed in.');
    expect(body).toContain('Turn off');
    expect(body).toContain('Keep on');
    expect(body).toContain('Their sign-ins end now, everywhere');
  });
});

describe('Settings and who is looking', () => {
  it('shows the Staff panel to an admin, before retention', () => {
    const { body } = render(Settings, { props: { me: asMorag, initial: null, initialHolds: null } });
    const drawn = [...body.matchAll(/data-panel="([^"]+)"/g)].map((m) => m[1]);
    expect(drawn[0]).toBe('settings.staff');
    expect(body).toContain('Loading retention settings…');
  });

  it('tells a presenter that retention and holds are an admin’s, with no refused request', () => {
    const { body } = render(Settings, { props: { me: asFraser, initial: null, initialHolds: null } });
    const drawn = [...body.matchAll(/data-panel="([^"]+)"/g)].map((m) => m[1]);
    expect(drawn).toEqual([]);
    expect(body).toContain('Only an admin changes how long the station keeps its records');
    expect(body).not.toContain('Loading retention settings');
  });

  it('with no answer yet, draws the retention panels as before', () => {
    const { body } = render(Settings, { props: { me: undefined, initial: null, initialHolds: null } });
    expect(body).toContain('Loading retention settings…');
    expect(body).not.toContain('settings.staff');
    expect(SETTINGS.map((p) => p.name)).not.toContain('settings.staff');
  });
});
