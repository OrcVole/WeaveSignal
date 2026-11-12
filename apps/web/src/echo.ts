// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import { startBrand } from './lib/brand.svelte.ts';
import { followSystemTheme } from './theme.ts';
import { reloadWhenStale } from './lib/stale.ts';
import EchoTest from './EchoTest.svelte';

reloadWhenStale();
followSystemTheme();
startBrand();
mount(EchoTest, { target: document.getElementById('app')! });
