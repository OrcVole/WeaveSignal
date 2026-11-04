// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import { followSystemTheme } from './theme.ts';
import Join from './Join.svelte';

followSystemTheme();
mount(Join, { target: document.getElementById('app')! });
