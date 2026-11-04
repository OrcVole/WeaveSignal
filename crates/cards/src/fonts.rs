// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The faces every card is set in, bundled so every server draws the same card (ADR-0053).
//!
//! Noto Sans 2.015, Noto Sans Arabic 2.013 and Noto Sans Mono 2.014, the unhinted TTFs from the
//! Noto Project's releases (`github.com/notofonts/latin-greek-cyrillic` and `.../arabic`),
//! Copyright 2022 The Noto Project Authors, SIL Open Font License 1.1 (`LICENSES/OFL-1.1.txt`,
//! recorded in `NOTICES.md`). They are embedded unmodified; OFL 1.1 allows bundling them with
//! software under any licence, and they are not sold on their own.

pub(crate) const FACES: [&[u8]; 7] = [
    include_bytes!("../fonts/NotoSans-Regular.ttf"),
    include_bytes!("../fonts/NotoSans-SemiBold.ttf"),
    include_bytes!("../fonts/NotoSans-Bold.ttf"),
    include_bytes!("../fonts/NotoSansArabic-Regular.ttf"),
    include_bytes!("../fonts/NotoSansArabic-SemiBold.ttf"),
    include_bytes!("../fonts/NotoSansArabic-Bold.ttf"),
    include_bytes!("../fonts/NotoSansMono-Regular.ttf"),
];
