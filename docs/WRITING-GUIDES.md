<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->

# Writing the simple guides

The simple guides (`src/content/docs/guides/`, published at `/guides/`) are for people, not for
programs or agents: a presenter before their first show, a guest with a link, a volunteer who
looks after the station. The full documentation under `/docs/` stays the complete record; a guide
is the short road through it. These rules adapt the estate's documentation contract (AIo v3) to
human readers.

## The rules

1. **Three steps to the answer.** The site's front page links to *Start here*; *Start here* asks
   who you are; each answer is one guide. A guide that needs a second guide to make sense is two
   guides badly cut.
2. **One guide, one job.** The title is the job in the reader's words ("Invite a guest", never
   "Invitations"). The first line says what you will have when you finish.
3. **Short.** One screen where it can be, two at most. Seven numbered steps at most; a longer job
   is two guides.
4. **Plain words.** No decision numbers (ADR-…), no API routes, no settings keys, no internal
   names (`desk.stage`, `control.toml`), no "the control plane". Say what the person sees on the
   screen, with the button's words in **bold**, exactly as the app writes them. A studio word the
   reader may not know (cue, mix-minus, playout) is explained in the same sentence or linked to
   *Words you will meet* once.
5. **Say what goes wrong.** Each guide ends with **If it does not work**: the two or three things
   that really happen, and what to do.
6. **Point to the detail.** Each guide ends with one **More detail** link to the full page under
   `/docs/` that it shortens. Never copy a full page's detail into a guide.
7. **True on the day.** Every guide carries `last_verified` and is checked against the running
   app before that date moves. When the app changes, the guide is rewritten, not given a
   "Note: from 1.0.2…" paragraph (supersede, do not append).
8. **Same house style as the rest.** Frontmatter as the other pages (`title`, `description`,
   `type: howto`, `status`, `last_verified`), semantic line breaks (one sentence per line),
   `just md-lint` clean, UK English, the noun is *invitation*, never *invite*.

## The set

| Guide | For |
|---|---|
| Start here | everyone |
| What WeaveSignal is | everyone |
| Your first show | a presenter or DJ |
| Invite a guest | a presenter |
| You have been invited | a guest |
| Take a phone caller | a presenter |
| Hand over to the next DJ | a presenter or DJ |
| Get your show ready while someone else is on air | the next DJ |
| Change the station's settings | whoever looks after the station |
| When something goes wrong | everyone |
| Words you will meet | everyone |
