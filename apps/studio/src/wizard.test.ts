// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The DJ's show setup (ADR-0218, ADR-0219), tested without a page: whose show a show is and
// when the wizard opens by itself, the answers checked and turned into a show, the usual
// slot from a day and two times, and the wizard, the Settings panel and the show editor
// rendered on the server. Sample people come from the Scottish names list (ADR-0071).
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { PANELS, SHOW_SETUP, panel } from '../../../packages/ui/panels.ts';
import type { Me } from './lib/api.ts';
import type { Show, ShowView } from './lib/schedule.ts';
import {
  aboutProblems,
  freeSlug,
  fromShow,
  minutesOf,
  myShow,
  needsSetup,
  nextColour,
  showProblems,
  slotFrom,
  overnight,
  slotLine,
  splitLinks,
  toShowInput,
  type Answers,
} from './lib/wizard.ts';
import MyShow from './settings/MyShow.svelte';
import ShowWizard from './settings/ShowWizard.svelte';
import { STEP_HINT, STEP_QUESTION, startsAt } from './lib/wizard.ts';
import ShowEditor from './schedule/ShowEditor.svelte';

const shona: Me = {
  kind: 'staff',
  role: 'presenter',
  account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'presenter', disabled_ms: null, created_ms: 1 },
};
const morag: Me = {
  kind: 'staff',
  role: 'admin',
  account: { id: 'a-morag', username: 'morag', name: 'Morag Campbell', role: 'admin', disabled_ms: null, created_ms: 1 },
};
const key: Me = { kind: 'station', role: 'admin' };

const show = (over: Partial<Show> = {}): Show => ({
  id: 'id-night',
  slug: 'the-night-show',
  name: 'The Night Show',
  description: '',
  colour_slot: 4,
  layout: 'poster',
  slot: { weekday: 5, start: 22 * 60, minutes: 120 },
  hosts: [],
  tags: [],
  explicit: false,
  feed_guid: null,
  cards: null,
  presenter: 'Shona Campbell',
  contact: 'shona@example.org',
  links: ['https://rumble.com/c/shona', 'https://odysee.com/@shona'],
  commercial_music: true,
  presenter_account: 'a-shona',
  ...over,
});

const answers = (over: Partial<Answers> = {}): Answers => ({
  presenter: 'Shona Campbell',
  contact: 'shona@example.org',
  show: 'The Night Show',
  links: 'https://rumble.com/c/shona\nhttps://odysee.com/@shona',
  weekday: 5,
  from: '22:00',
  until: '00:00',
  commercial_music: true,
  // The show fixture is from before steps: it has no step, which reads as Full.
  step: 'full',
  ...over,
});

describe('whose show, and when the wizard opens by itself', () => {
  it('finds the show tied to the account, and none for the station key', () => {
    const mine = show();
    const other = show({ id: 'id-other', slug: 'other', presenter_account: 'a-morag' });
    expect(myShow(shona, [other, mine])?.slug).toBe('the-night-show');
    expect(myShow(morag, [other, mine])?.slug).toBe('other');
    expect(myShow(key, [other, mine])).toBeNull();
    expect(myShow(shona, [show({ presenter_account: null })])).toBeNull();
  });

  it('opens for a presenter with no show of their own, unless skipped', () => {
    expect(needsSetup(shona, [], false)).toBe(true);
    expect(needsSetup(shona, [], true)).toBe(false);
    expect(needsSetup(shona, [show()], false)).toBe(false);
    // Admins, producers and screeners find it in Settings; the key has no account.
    expect(needsSetup(morag, [], false)).toBe(false);
    expect(needsSetup(key, [], false)).toBe(false);
    expect(needsSetup(null, [], false)).toBe(false);
  });
});

describe('the answers', () => {
  it('start from the show when run again, else from the account', () => {
    expect(fromShow(show(), shona)).toEqual(answers());
    expect(fromShow(null, shona)).toMatchObject({ presenter: 'Shona Campbell', contact: '', show: '', links: '', weekday: 6, from: '', until: '' });
  });

  it('start a new show at Solo, and keep the step of a show that has one (ADR-0271)', () => {
    expect(fromShow(null, shona).step).toBe('solo');
    expect(fromShow(show({ step: 'guests' }), shona).step).toBe('guests');
    // A show from before steps says none: everything, as it always ran.
    expect(fromShow(show(), shona).step).toBe('full');
    expect(toShowInput(answers({ step: 'callers' }), shona, null, 1).step).toBe('callers');
  });

  it('say where the studio starts, for each step', () => {
    expect(startsAt('solo')).toEqual({
      lead: 'The studio starts at Solo:',
      rest: 'your microphone, the music and carts, the listener count and the chat. Guests, callers and the rest are switched off until you move up a step, at the top of the studio.',
    });
    expect(startsAt('guests').rest).toMatch(/guests you invite by link\. Callers and the rest are switched off until you move up a step/);
    expect(startsAt('callers').rest).toMatch(/listeners who call in.*The screener’s seat, the picture and the handover are switched off/);
    expect(startsAt('full')).toEqual({ lead: 'The studio starts at Full:', rest: 'the whole studio, with a screener on the phones, the picture and the next DJ. Nothing is switched off.' });
  });

  it('read a time, and make a slot that may run past midnight', () => {
    expect(minutesOf('20:00')).toBe(1200);
    expect(minutesOf('9:05')).toBe(545);
    expect(minutesOf('24:00')).toBeNull();
    expect(minutesOf('tea time')).toBeNull();
    expect(slotFrom({ weekday: 6, from: '20:00', until: '23:00' })).toEqual({ weekday: 6, start: 1200, minutes: 180 });
    // An end before the start is a past-midnight slot the wizard asks to confirm (ADR-0244).
    expect(overnight({ from: '20:00', until: '19:00' })).toBe(true);
    expect(overnight({ from: '22:00', until: '01:00' })).toBe(true);
    expect(overnight({ from: '20:00', until: '23:00' })).toBe(false);
    expect(overnight({ from: '20:00', until: '' })).toBe(false);
    expect(slotFrom({ weekday: 5, from: '22:00', until: '00:00' })).toEqual({ weekday: 5, start: 1320, minutes: 120 });
    expect(slotFrom({ weekday: 5, from: '22:00', until: '' })).toEqual({ weekday: 5, start: 1320, minutes: null });
    expect(slotFrom({ weekday: 5, from: '', until: '' })).toBeNull();
    expect(slotLine({ weekday: 6, from: '20:00', until: '23:00' })).toBe('Saturdays, 20:00 to 23:00');
    expect(slotLine({ weekday: 1, from: '', until: '' })).toBe('No usual time yet');
  });

  it('are checked step by step, with the field named', () => {
    expect(aboutProblems(answers())).toEqual({});
    expect(aboutProblems(answers({ presenter: '  ' })).presenter).toMatch(/name you go by/);
    expect(aboutProblems(answers({ contact: 'x'.repeat(201) })).contact).toMatch(/200/);
    // The contact is optional: the station's own is said alone (ADR-0215).
    expect(aboutProblems(answers({ contact: '' }))).toEqual({});
    expect(showProblems(answers())).toEqual({});
    expect(showProblems(answers({ show: '' })).show).toMatch(/show's name/);
    expect(showProblems(answers({ links: 'not a link' })).links).toMatch(/no spaces/);
    expect(showProblems(answers({ links: Array.from({ length: 11 }, (_, i) => `https://x.example/${i}`).join('\n') })).links).toMatch(/10 links/);
    expect(showProblems(answers({ from: '', until: '23:00' })).time).toMatch(/start as well/);
    expect(showProblems(answers({ from: '25:00' })).time).toMatch(/such as 20:00/);
  });

  it('split links on lines and commas, and keep trimming', () => {
    expect(splitLinks(' https://a.example \n\nhttps://b.example, https://c.example')).toEqual(['https://a.example', 'https://b.example', 'https://c.example']);
  });

  it('become the PUT body: a new show tied to the account, an old one keeping what it had', () => {
    const fresh = toShowInput(answers(), shona, null, nextColour(3));
    expect(fresh).toEqual({
      name: 'The Night Show',
      description: '',
      colour_slot: 4,
      layout: 'poster',
      slot: { weekday: 5, start: 1320, minutes: 120 },
      hosts: [],
      tags: [],
      explicit: false,
      presenter: 'Shona Campbell',
      contact: 'shona@example.org',
      links: ['https://rumble.com/c/shona', 'https://odysee.com/@shona'],
      commercial_music: true,
      presenter_account: 'a-shona',
      step: 'full',
    });
    const existing: ShowView = {
      show: show({ description: 'Late calls.', colour_slot: 9, layout: 'slate', tags: ['calls'], explicit: true }),
      resolved: { hosts: ['Catriona MacKenzie-Buchanan'] } as ShowView['resolved'],
      series: [],
    };
    const again = toShowInput(answers({ show: 'The Later Show' }), shona, existing, 1);
    expect(again).toMatchObject({ name: 'The Later Show', description: 'Late calls.', colour_slot: 9, layout: 'slate', tags: ['calls'], explicit: true, hosts: ['Catriona MacKenzie-Buchanan'] });
    expect(nextColour(0)).toBe(1);
    expect(nextColour(12)).toBe(1);
  });

  it('take a free path for a new show', () => {
    expect(freeSlug('The Night Show', [])).toBe('the-night-show');
    expect(freeSlug('The Night Show', ['the-night-show'])).toBe('the-night-show-2');
    expect(freeSlug('The Night Show', ['the-night-show', 'the-night-show-2'])).toBe('the-night-show-3');
    expect(freeSlug('!!!', [])).toBe('show');
  });
});

describe('the wizard rendered', () => {
  const props = { me: shona, shows: [] as Show[], onDone: () => {}, onSkip: () => {}, onClose: () => {} };

  it('is a named panel with its three parts, the first asking only the on-air name and the contact', () => {
    const { body } = render(ShowWizard, { props });
    expect(body).toContain('data-panel="studio.show-setup"');
    expect(SHOW_SETUP.map((p) => p.name)).toContain('studio.show-setup');
    expect(PANELS['studio.show-setup']).toBeTruthy();
    expect(body).toContain('Set up your show');
    expect(body).toContain('Part 1 of 3: About you');
    expect(body).toContain('Your on-air name');
    expect(body).toContain('Takedown contact');
    expect(body).toContain('then\n          the station’s own as the host');
    expect(body).toContain('Skip for now');
    expect(body).toContain('Next: your show');
    // The account's name is offered as the on-air name.
    expect(body).toContain('value="Shona Campbell"');
    // No email form anywhere (ADR-0218): the contact may be an address, but nothing is sent.
    expect(body).not.toContain('type="email"');
    // "Step" means the show's step and nothing else: the wizard's pages are parts.
    expect(body).not.toMatch(/Step \d of 3/);
  });

  it('asks on its second part for the name, the links, the usual day and time, and the music reminder', () => {
    const { body } = render(ShowWizard, { props: { ...props, part: 2 } });
    expect(body).toContain('Part 2 of 3: Your show');
    expect(body).toContain('Show name');
    expect(body).toContain('Links to your channels');
    expect(body).toContain('Usual day and time');
    expect(body).toContain('>Saturday<');
    expect(body).toContain('The show plays commercial music');
    expect(body).toContain('Save my show');
    expect(body).not.toContain('PPL'); // the reminder only once the box is ticked
  });

  it('asks how much of the studio to start with, with Solo picked, in the step list every surface shares', () => {
    const { body } = render(ShowWizard, { props: { ...props, part: 2 } });
    expect(body).toContain(STEP_QUESTION);
    expect(STEP_QUESTION).toBe('How much of the studio to start with');
    expect(body).toContain(STEP_HINT);
    const list = body.slice(body.indexOf('class="steplist'), body.indexOf('</fieldset>', body.indexOf('class="steplist')));
    expect([...list.matchAll(/data-step="(\w+)"/g)].map((m) => m[1])).toEqual(['solo', 'guests', 'callers', 'full']);
    expect(list).toMatch(/<input[^>]*value="solo"[^>]*checked/);
    expect(list).not.toMatch(/<input[^>]*value="(guests|callers|full)"[^>]*checked/);
    expect(list).toContain('Just you');
    expect(list).toContain('A screener on the phones, the picture, the next DJ');
    // The wizard keeps three pages.
    expect(body).toContain('1 About you');
    expect(body).toContain('3 Done');
    expect(body).not.toContain('4 ');
  });

  it('says on its last part where the studio starts and where to move up', () => {
    const { body } = render(ShowWizard, { props: { ...props, part: 3 } });
    expect(body).toContain('The studio starts at Solo:');
    expect(body).toContain('until you move up a step, at the top of the studio.');
  });

  it('runs again from an existing show with its answers and the reminder lit', () => {
    const { body } = render(ShowWizard, { props: { ...props, initial: show(), part: 2 } });
    expect(body).toContain('value="The Night Show"');
    expect(body).toContain('https://rumble.com/c/shona');
    expect(body).toContain('Fridays, 22:00 to 00:00');
    expect(body).toContain('PPL and PRS for Music');
    expect(body).toContain('Save changes');
    expect(body).not.toContain('Skip for now');
  });
});

describe('Settings › Your show', () => {
  it('shows the account’s show as the wizard left it, under its panel name', () => {
    const { body } = render(MyShow, { props: { me: shona, initial: [show()] } });
    expect(body).toContain('data-panel="settings.my-show"');
    expect(panel('settings.my-show').what).toMatch(/Your show/);
    expect(body).toContain('The Night Show');
    expect(body).toContain('Shona Campbell');
    expect(body).toContain('shona@example.org');
    expect(body).toContain('then the station’s as the host');
    expect(body).toContain('https://odysee.com/@shona');
    expect(body).toContain('Fridays, 22:00 to 00:00');
    expect(body).toContain('PPL and PRS for Music');
    expect(body).toContain('Change…');
  });

  it('invites a member of staff with no show to set one up, without email', () => {
    const { body } = render(MyShow, { props: { me: morag, initial: [show()] } });
    expect(body).toContain('You have not set up a show yet');
    expect(body).toContain('Set up your show…');
    expect(body).toContain('nothing is emailed');
  });
});

describe('the show editor', () => {
  it("carries the presenter's fields as a named panel (ADR-0214, ADR-0219)", () => {
    const { body } = render(ShowEditor, { props: { slug: null, show: null, onSaved: () => {}, onCard: () => {} } });
    expect(body).toContain('data-panel="calendar.show-presenter"');
    expect(panel('calendar.show-presenter').what).toMatch(/takedown contact/);
    expect(body).toContain('On-air name');
    expect(body).toContain('Takedown contact');
    expect(body).toContain('Channel links');
    expect(body).toContain('Commercial music');
    expect(body).toContain('it blocks nothing');
  });
});
