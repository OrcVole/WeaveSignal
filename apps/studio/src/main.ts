// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { mount } from 'svelte';
import '../../../packages/ui/tokens.css';
import './styles/base.css';
import './styles/desk-b.css';
import './styles/screener.css';
import './styles/carts.css';
import './styles/schedule.css';
import './styles/steps.css';
import './styles/prepare.css';
import './app.css';
import App from './App.svelte';
import { applyTheme, theme } from './lib/theme.svelte.ts';
import { loadStation } from './lib/api.ts';

applyTheme(theme.current);
// The packaged app reads its station from the Rust side first (ADR-0121).
void loadStation().finally(() => mount(App, { target: document.getElementById('app')! }));
