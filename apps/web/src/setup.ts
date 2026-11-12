// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's setup page (ADR-0237). It keeps the product's own look: it is the admin's tool,
// not the station's shop window, so the station's branding is previewed on it, never applied.
import { mount } from 'svelte';
import { followSystemTheme } from './theme.ts';
import { reloadWhenStale } from './lib/stale.ts';
import Setup from './Setup.svelte';

reloadWhenStale();
followSystemTheme();
mount(Setup, { target: document.getElementById('app')! });
