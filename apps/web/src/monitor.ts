// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor link (`/monitor/#<secret>`, ADR-0280): the page a host opens beside their own
// mixer during their show.
import { mount } from 'svelte';
import { startBrand } from './lib/brand.svelte.ts';
import { reloadWhenStale } from './lib/stale.ts';
// The tokens and the public pages' stylesheet come with the theme module; this page sets the
// theme itself, because it adds Night to the device's light or dark.
import './theme.ts';
import './monitor.css';
import { followDeviceOrNight } from './lib/monitor/night.svelte.ts';
import Monitor from './Monitor.svelte';

reloadWhenStale();
followDeviceOrNight();
startBrand();
mount(Monitor, { target: document.getElementById('app')! });
