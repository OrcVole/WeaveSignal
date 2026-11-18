// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The studio app's own version, for the frame (the operator, 2026-11-17: "the current version of
// weavesignal indicated on the top-bar"). It is read at build time from the file Tauri takes the
// packaged app's version from (`package_info().version`, which Settings › Updates shows and the
// update feed compares), so the frame and that screen cannot disagree, and a browser build or the
// sandbox, which have no Tauri to ask, say it too. Only the one field is taken into the bundle.
//
// Kept outside `lib/`: the Cloudron image copies `apps/studio/src/lib` alone for the public
// pages' build, and this file reaches outside it.
import { version } from '../src-tauri/tauri.conf.json';

export const APP_VERSION: string = version;
