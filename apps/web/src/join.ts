// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import { startBrand } from './lib/brand.svelte.ts';
import { followSystemTheme } from './theme.ts';
import { reloadWhenStale } from './lib/stale.ts';
import Join from './Join.svelte';

reloadWhenStale();
followSystemTheme();
startBrand();
mount(Join, { target: document.getElementById('app')! });
