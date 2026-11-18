// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What `/` shows on the station's own address until the station is set up (1.1): the station
// serves this page in place of the player (the control plane on Compose, nginx on Cloudron).
import { mount } from 'svelte';
import { startBrand } from './lib/brand.svelte.ts';
import { followSystemTheme } from './theme.ts';
import NotSetUp from './NotSetUp.svelte';

followSystemTheme();
startBrand();
mount(NotSetUp, { target: document.getElementById('app')! });
