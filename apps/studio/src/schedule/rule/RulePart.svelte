<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // When a show airs (`calendar.rule`; ADR-0278, new-show.md section 1): the questions on the
  // left, and on the right what they make, said back by the control plane. Built once and
  // hosted three times: part 2 of New show, part 3 of the DJ's setup, and the rule dialog.
  // The studio works out no dates (section 8): 250 ms after the last change it asks the
  // station for a preview, and drops any answer older than the newest request.
  import { onDestroy, untrack } from 'svelte';
  import * as api from '../../lib/api.ts';
  import type { IsoDate, RulePreview, RulePreviewRequest } from '../../lib/api.ts';
  import { account } from '../../lib/account.svelte.ts';
  import RuleQuestions from './RuleQuestions.svelte';
  import RulePreviewPane from './RulePreview.svelte';
  import { dropGone, hhmm, previewRequest, rememberMode, times, toggleDate, withMove, withSkip, withoutMove, withoutSkip, type RuleForm } from './form.ts';
  import { Previewer, type PreviewState } from './previewer.ts';

  let {
    form = $bindable(),
    state: st = $bindable(),
    zone,
    today,
    zoneSet = true,
    canChooseZone = false,
    onzone,
    rules = true,
    show = null,
    series = null,
    colour = null,
    label = 'First on',
    countHint = '',
    idp = 'rq',
    live = true,
    ask = api.previewRule,
    moving = null,
    seeded = null,
    onunseed,
  }: {
    form: RuleForm;
    /** What the station last said of the answers. The host holds it, seeded with a first
     *  answer if it has one, and reads it to word and enable its buttons (`ruleStatus`). */
    state: PreviewState;
    zone: string;
    /** The station's date today. */
    today: IsoDate;
    zoneSet?: boolean;
    canChooseZone?: boolean;
    onzone?: () => void;
    /** The station stores rules (1.0.4 or later); an older one keeps a weekly time only. */
    rules?: boolean;
    /** The show and series, when they exist: their own episodes are not clashes. */
    show?: string | null;
    series?: number | null;
    colour?: number | null;
    label?: string;
    countHint?: string;
    idp?: string;
    /** Ask the station; off where there is none (a test's render). */
    live?: boolean;
    ask?: (req: RulePreviewRequest) => Promise<RulePreview>;
    moving?: IsoDate | null;
    /** New show only: First on and from came from the block selected in the week. */
    seeded?: { words: string; clear: string } | null;
    onunseed?: () => void;
  } = $props();

  // An answer the host already has is not asked for again.
  const seed = untrack(() => ({ initial: st.answer, live, ask }));
  let shown = $state(10);
  let droppedWords = $state('');
  /** The date last moved, so a move the station refuses can be taken back, and why it was. */
  let lastMove: IsoDate | null = null;
  let moveRefused = $state('');
  /** A session the station refuses a clash (a presenter's, a screener's): only the wording of a
   *  later date's clash follows this; what is refused is the station's to say. */
  const mayRefuse = $derived(account.me?.kind === 'staff' && (account.me.role === 'presenter' || account.me.role === 'screener'));

  const previewer = new Previewer(
    (req) => seed.ask(req),
    (s) => {
      st = s;
      // Skipped and moved dates the new pattern no longer gives are dropped, and said once.
      const gone = s.answer && !s.stale ? s.answer.dropped : null;
      if (gone && (gone.skip.length || gone.moves.length)) {
        const d = dropGone(untrack(() => form), gone);
        if (d.words) {
          droppedWords = d.words;
          form = d.form;
        }
      }
      // A move the station refuses (it must stay between its neighbours) is taken back, with
      // the station's reason, so the dates come back and the person can try another.
      const refusal = s.answer && !s.stale ? s.answer.problems.find((p) => p.field === 'moves') : undefined;
      if (refusal && lastMove) {
        const on = lastMove;
        lastMove = null;
        moveRefused = `Not moved: ${refusal.message.charAt(0).toLowerCase()}${refusal.message.slice(1)}${/[.!?]$/.test(refusal.message) ? '' : '.'}`;
        form = withoutMove(untrack(() => form), on);
      }
    },
  );
  previewer.seed(seed.initial);
  onDestroy(() => previewer.stop());

  const request = $derived(rules ? previewRequest(form, { show, series, limit: shown }) : null);

  let first = true;
  $effect(() => {
    const req = request;
    if (!seed.live || !rules) return;
    // The first answer is asked for at once, unless it was given; after that, 250 ms after the last change.
    const now = first && !seed.initial;
    const given = first && !!seed.initial;
    first = false;
    if (given) return;
    untrack(() => previewer.changed(req, now));
  });

  /** A change the person made: what was said once about dropped dates has been read. */
  function change(next: RuleForm) {
    droppedWords = '';
    moveRefused = '';
    lastMove = null;
    form = next;
  }

  function move(on: IsoDate, to: IsoDate, at: string) {
    change(withMove(form, on, to, at));
    lastMove = on;
  }

  /** 1 rem in the pixels `getBoundingClientRect` gives under the page's zoom. The two columns
   *  need 56 rem; a container query cannot say so, because it multiplies its rem by <html>'s
   *  zoom and measures the container without it (WebKitGTK 4.1 and Chromium: at 110 % the
   *  widest dialog stacked, its dates out of sight; the 1.0.4 test run). */
  function remPx(): number {
    const p = document.createElement('div');
    p.style.cssText = 'position:fixed;left:0;top:0;width:1rem;height:0;visibility:hidden;pointer-events:none';
    document.body.append(p);
    const w = p.getBoundingClientRect().width;
    p.remove();
    return w || 16;
  }
  let ruleEl = $state<HTMLDivElement>();
  let narrow = $state(false);
  $effect(() => {
    const el = ruleEl;
    if (!el) return;
    const measure = () => (narrow = el.getBoundingClientRect().width < 56 * remPx());
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  });

  function tryTime(start: number) {
    const t = times(form);
    change({ ...form, from: hhmm(start), until: hhmm(start + (t?.minutes ?? 60)), overnightOk: start + (t?.minutes ?? 60) >= 1440 });
  }
</script>

<div class="rulepart" class:bare={!rules} class:narrow bind:this={ruleEl} data-panel="calendar.rule">
  <RuleQuestions
    {form}
    onchange={change}
    {label}
    {zone}
    {zoneSet}
    {canChooseZone}
    {onzone}
    old={!rules}
    problems={st.stale ? [] : (st.answer?.problems ?? [])}
    hints={st.stale ? [] : (st.answer?.hints ?? [])}
    {countHint}
    onmode={(m) => rememberMode(account.me?.account?.id, m)}
    {idp}
    {seeded}
    {onunseed}
  />
  {#if rules}
    <RulePreviewPane
      state={st}
      {form}
      {zone}
      {today}
      {colour}
      dropped={[droppedWords, moveRefused].filter(Boolean).join(' ')}
      {mayRefuse}
      {moving}
      onskip={(on) => change(withSkip(form, on))}
      onputback={(on) => change(withoutSkip(form, on))}
      onmove={move}
      onunmove={(on) => change(withoutMove(form, on))}
      onpick={(iso) => change(toggleDate(form, iso))}
      onmore={() => (shown = Math.min(60, shown + 10))}
      ontry={tryTime}
      onretry={() => previewer.retry()}
    />
  {/if}
</div>
