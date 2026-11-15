---
title: "Schedule the week"
description: "Make a show with its weekly rule, draft its episodes from the rule, move one, set a field on the episode or let it inherit, check a card, and read the generated hours between shows."
type: howto
status: draft
last_verified: 2026-11-15
sidebar:
  order: 22
---

This is for the station manager, on the studio app's Schedule tab (`⌃5`).

## Choose the station's time zone first

Everything on the tab is in the station's own time zone, kept in the station's database and chosen once in **Settings**, under **Station time zone**.
Until someone chooses it, the station offers `publishing.zone` from `control.toml` (Europe/London unless you changed it), and the toolbar shows **Choose the time zone…** to an admin.

1. Select **Choose the time zone…** in the toolbar, or open **Settings**.
2. Under **Station time zone**, keep the zone offered or type another IANA name in **Time zone**, such as `Europe/London` or `America/New_York`.
3. Select **Use Europe/London** (the button names the zone offered), or **Use this time zone** for one you typed. The toolbar's prompt goes.

Automation's clock hours use the same zone. The grid shows it in the toolbar, and a clocks-change Sunday shows its 23 or 25 hours.

![Settings, Station time zone: before anyone chooses a zone the station says so and offers its configured one.](../../../../assets/screens/how-to-schedule-the-week-time-zone.png)

Change the zone later only if you must. Shows' rules and clock hours are stored on the wall clock with no zone, so after a change each airs at the same hour in the new zone, which is a different moment. Settings warns you before you select **Change time zone**.

## Make a show

![New show, part 2, When it airs: the questions on the left, and on the right the sentence, the small months and the next dates.](../../../../assets/screens/how-to-schedule-the-week-new-show.png)

1. Select **New show** in the toolbar. If a block was selected, the date and time start from it; change them if that is not what you want.
2. **The show**: give it a **Name**, pick its **Colour** (a bank slot: the same colour on the cart wall and the cards, ADR-0066) and its step, then **Next: when it airs**.
3. **When it airs**: give **First on** and the times **from** and **until**, in the station's zone. A show that runs past midnight is asked about once.
4. Under **Repeats**, pick the sentence that fits: *Once only*, *Every week on Tuesday*, *Every month on the third Tuesday*, *Every year on 17 November*, or **On dates I choose…** (click the dates in the small months; **Remove** takes one out). **More choices…** gives every other week, several weekdays, the last Friday of the month, a day of the month, and the rest.
5. Under **Ends**, pick **Never**, **On** a date, or **After** a number of episodes.
6. Read the right-hand side: the rule in words with its last date, and the next dates with their episode numbers. **Skip** a date the show does not air (a skipped date does not count towards the episodes), or **Move…** one to another day or time.
7. If a date clashes with another show, its row says CONFLICT with the other show and the overlap. When every date clashes, the station offers the nearest clear time, such as **Try 21:00 to 22:00**.
8. Select **Make the show** (**Make the show anyway** while something clashes). The station draws the show's cards first; a name that does not fit the card, or holds characters its fonts cannot draw, is refused with the field named.
9. **Done** says which episode is drafted. **Draft all 10** drafts the rest of a rule with an end; **Open episode 1** selects it.

The other dates appear on the grid as dashed *Not drafted* blocks. **No dates yet** in step 7 makes the show without a rule, to give it dates later.

Hosts, a description, the card layout and the presenter's details are in the show editor; a series and its episodes inherit them until set lower down (ADR-0055).

A show made by mistake, or a test show, can be removed again while none of its episodes has aired: [Delete a show](/docs/how-to/delete-a-show/).

## Change when a show airs

Select one of its blocks, then **Show**, and at **When it airs** select **Change…**. The same questions open with the show's rule. **Save…** lists the drafted episodes with their old and new dates: **Move them to the new dates** (the default) or **Leave them where they are**. Aired episodes never move. Every window is told who changed it.

## Draft the episodes

- Select a dashed block and **Draft this episode**: the episode takes its number (the last plus one) and the time from the rule. **Skip this date** takes that date out instead (**Undo** puts it back).
- Or open any episode of the show, switch to **Series n**, set **Starts on** and **Planned episodes**, **Save series**, and **Draft the season**: every planned episode not yet drafted, each on its instance of the rule, across the clock change.
- **Create next episode** on either level drafts one more.

![A dashed rule date selected: Kettle Breakfast, not drafted, with Draft this episode.](../../../../assets/screens/how-to-schedule-the-week-draft-from-rule.png)

Drafting never touches an aired episode.

## Mark an episode as aired

When the show goes out, select its episode and **On air now**: the episode is frozen as broadcast and its cards are drawn once more.
Nothing does this by itself yet; the desk going on air does not.

## Change an episode

![An episode's editor: each field says whether it is inherited or set here.](../../../../assets/screens/reference-calendar-episode-editor.png)

Select its block.
Each field says where its value comes from:

| Marker | Means | Actions |
|---|---|---|
| **from Series 3**, **from the show**, **from the series rule** (dotted) | the field follows the level above; change the series' hosts and this episode follows | **Set here** copies the value down and makes it the episode's own |
| **set here** (accent) | the episode's own | **Clear**, or **Reset to series** to inherit again |
| **as aired** (a lock) | the episode has aired and is frozen; what was broadcast is not rewritten | none |

Guests are always the episode's own.
**Kind** is **Live** or **Pre-recorded** (ADR-0188); it is the episode's own, and every episode is live until you mark it. It is kept in the schedule and frozen with the episode when it airs.
**Save episode** sends only what changed.
A title, guest or host name that holds characters the card fonts cannot draw is refused with the field named, and nothing is saved (ADR-0176).

To move an episode, drag its block to another time or pull its end, or **Move…** on the date field and set the time and length by hand; a moved instance says MOVED · was 07:00 on the grid, and **Back to the rule** undoes it.
If the control plane refuses a move, the block snaps back and the toast says why.

Two episodes that overlap both say CONFLICT.
Automation never conflicts: it fills whatever the shows leave.

## Correct an aired episode

An aired episode is frozen so the archive stays true to what was broadcast, but a misspelt title or guest can be put right.
On the aired episode, **Correct the public text…** under **Corrections**: change the title, description, hosts, guests or tags, say **Why** in a sentence, and **Publish correction**.
An admin can, and so can the member of staff who made the episode (ADR-0198); anyone else is told to ask one of them. An episode made before WeaveSignal recorded makers (November 2026), or made with the station key or an API token, is an admin's to correct. The log names who did it.
The cards are drawn again from the corrected text, the podcast feed shows the new text under the same episode id (so apps do not list it twice), and the published audio files are tagged again with the new title and card.
Nothing is changed if the cards cannot be drawn: the correction is refused with the field named.

![An aired episode's Corrections: one correction already in the log with its reason, and a second being made with Why filled in.](../../../../assets/screens/how-to-schedule-the-week-correction.png)

The frozen record is not rewritten: each correction is a line in the episode's log with who made it, when, why, and the fields as they were and as they became.
The air time, the length and the kind are not text and cannot be corrected.
If a person asks to be erased ([Set retention](/docs/how-to/set-retention/)), their name leaves the log as well: the line stays, marked as redacted, with the name taken out of its old and new values and its reason.

## Check the card

![The card editor with the Poster layout drawn for the episode.](../../../../assets/screens/reference-cards-poster-editor.png)

**Card…** on the artwork field (or on the show level) opens the card editor: switch Poster and Slate, the link card, the post image and the podcast square, and read the alt text that travels with the card and the page it links to.
A field that does not fit, or that holds characters the card fonts cannot draw (they cover Latin, Greek, Cyrillic and Arabic; Chinese, Japanese, Korean, Hebrew and Devanagari names are not covered), is named there before you save, and the save is refused (ADR-0176).
**Keep for the show** saves a tried layout.

## Read the generated hours

Select an automation block: the log for those hours, each track with the rule it breaks, if any, in the scheduler's words.
**Another track…** picks a replacement from the library, and **Use it** puts it in.
If the control plane refuses it, the reason is shown: the station's own separations can be accepted with **Accept the break**, a break of the US statutory licence never.
**Remove** takes an item out.
An empty future hour offers **Generate these hours**; the control plane also keeps the log generated 26 hours ahead by itself.

## Keys

`←` `→` move a week, `T` is today, `Tab` reaches the blocks and `⏎` opens one; the global keys (`⌃1` to `⌃5`, `F1` to `F6`, `⌃T`) work as on every tab.
