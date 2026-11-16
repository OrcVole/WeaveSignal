---
title: "Delete a show"
description: "Delete a show made by mistake, or a test show, from the show editor: who may, what goes with it, why a show that has aired cannot be deleted, and what to do instead."
type: howto
status: draft
last_verified: 2026-11-13
sidebar:
  order: 23
---

A show can be deleted while none of its episodes has aired (ADR-0279, release 1.0.4).
Use it for a show made by mistake, or a test show.
Nothing brings a deleted show back.

## Who may

An **admin** or a **producer** may.
A presenter may not, even for their own show, and nor may a screener or an API token; the station answers them that only an admin or a producer deletes a show.
The station key counts as an admin.

## Delete a show

1. On the Schedule tab (`⌃5`), open the show: choose its row in the list of shows (**‹ Shows** comes back to the list from anywhere in the column), or select one of its episodes and choose **Show** in the level switch. It opens at **Dates**; choose **Details** for the show editor.
2. At the foot of the editor, on the left and away from **Save show**, select **Delete show…**. A station older than 1.0.4 does not offer it, and nor does the editor to anyone who may not.
3. The dialog is headed *Delete* and the show's name, and says what goes with it, with the counts it has read: *Its 2 series and 7 drafted episodes go with it, and its cart banks and cards. This cannot be undone.*
4. **Keep it** is the default, so `⏎` or `Esc` leaves the show as it was. Select **Delete show** to go on.
5. The editor closes, the week and the list of shows with nothing coming up are read again, and the tab says *The Folk Club is deleted.* There is no Undo.

## What goes with a show

All at once, or not at all:

- the show, its series and its drafted episodes;
- the show's own cart banks and the carts in them, and their audio files (the station's banks, which every show shares, stay);
- the logs of its step and rule changes;
- its cards (link, post and square), its own slate picture, and the published audio files only its episodes named.

What stays: the play log, the generated log and the chat name no show, so they are as they were.
A caller's consent keeps the show's name in the words they agreed to; only its link to the show is cut.

## Why a show that has aired cannot be deleted

An aired episode is the station's record: what it said as it went out, the corrections log, the archive and its retention, and the podcast feed (ADR-0055, ADR-0177).
So the station refuses, and the dialog says why and keeps the show: *The Folk Club has aired 3 episodes, so it stays as the station's record: only a show that has never aired can be deleted.*
It also refuses a show on air now, in a sentence of its own.

The station counts an episode as aired once its time has come, whether or not anyone pressed **On air now**, and refuses while one is under way.
So a show can be deleted only until its first date arrives.

To end a show that has aired, give it an end in [When a show airs](/docs/reference/show-rules/): the show stays, with nothing more to come.

## For integrations

`DELETE /api/v1/shows/{show}` does the same, with the same rules; no API token's scope opens it ([Control API](/docs/reference/control-api/), [API reference](/docs/reference/api/)).
