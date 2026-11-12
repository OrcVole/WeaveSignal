// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The Jurisdiction setting's presets (ADR-0113, ADR-0114) as the website's wizard offers them:
// keys and names only. The station is the authority: its setup page reads the list, with each
// preset's rules, from `/api/v1/retention`. A test in crates/model holds this list against the
// presets in the code (`the_wizards_preset_list_is_the_codes`), so the two cannot drift.
// Custom is left out: it is set period by period in the studio's Settings.
export const PRESETS: readonly { key: string; name: string; licensed: boolean }[] = [
  { key: 'none', name: 'No retention', licensed: false },
  { key: 'uk_ofcom', name: 'UK, Ofcom-licensed', licensed: true },
  { key: 'uk_internet', name: 'UK, internet-only', licensed: false },
  { key: 'us_webcaster', name: 'US webcaster (statutory licence)', licensed: false },
  { key: 'us_nce', name: 'US noncommercial educational webcaster', licensed: false },
  { key: 'eu_gdpr', name: 'EU, GDPR only', licensed: false },
  { key: 'ie_licensed', name: 'Ireland, licensed', licensed: true },
  { key: 'ie_internet', name: 'Ireland, internet-only (audio)', licensed: false },
  { key: 'de', name: 'Germany', licensed: true },
  { key: 'ca_licensed', name: 'Canada, licensed', licensed: true },
  { key: 'ca_internet', name: 'Canada, internet-only', licensed: false },
  { key: 'au_licensed', name: 'Australia, licensed', licensed: true },
  { key: 'au_internet', name: 'Australia, internet-only', licensed: false },
];
