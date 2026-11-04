// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import { followSystemTheme } from './theme.ts';
import EchoTest from './EchoTest.svelte';

followSystemTheme();
mount(EchoTest, { target: document.getElementById('app')! });
