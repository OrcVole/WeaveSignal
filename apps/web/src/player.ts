// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import { followSystemTheme } from './theme.ts';
import Player from './Player.svelte';

followSystemTheme();
mount(Player, { target: document.getElementById('app')! });
