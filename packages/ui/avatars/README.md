<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# The shipped chat avatars

Seventy-two avatars a chatter can choose without uploading anything (ADR-0235): 59 animals and
13 monsters, each a 128-pixel WebP of about 1 KB (83 KB in all).

| | |
|---|---|
| Set | Fluent Emoji, the Flat style |
| Author | Microsoft Corporation |
| Licence | MIT (`LICENSES/MIT.txt`); read from the repository's `LICENSE` on 2026-11-10 |
| Source | `https://github.com/microsoft/fluentui-emoji` |
| Commit | `1ffb34c752ecf5d402f04cfb4b392c77f57c54bc` (2026-08-24) |
| Fetched | 2026-11-10 |

Each picture is `assets/<source>/Flat/<source, lower case, underscores>_flat.svg` at that commit,
where `<source>` is the avatar's `source` in [`manifest.json`](manifest.json); for example the
fox is `assets/Fox/Flat/fox_flat.svg`.

**What was changed.** Each SVG was drawn at 92 pixels in the middle of a 128-pixel square of one
of eight soft colours, so it reads as an avatar in a light and a dark chat, and written as WebP.
The drawings themselves are untouched. The SVGs are not kept here: [`render.sh`](render.sh)
fetches them again at the pinned commit and draws every file.

**The list.** `manifest.json` names every avatar: its `id` (the file's name, and what the chat
keeps for a chatter who chose it), the `label` a screen reader says, and its `group`. The chat
service reads the same file (`crates/chat/src/avatar.rs`), so it accepts exactly these.

To add or drop one, edit `manifest.json`, run `render.sh`, and commit both.
