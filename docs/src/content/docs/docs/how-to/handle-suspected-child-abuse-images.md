---
title: "Handle a picture you suspect is child sexual abuse material"
description: "What to do when a picture a chatter uploaded looks like child sexual abuse material: refuse it with its own reason, let the station keep it, report it, and what never to do."
type: howto
status: draft
last_verified: 2026-11-10
sidebar:
  order: 12
---

This is a runbook, not legal advice, and nothing here says a station is compliant with any law.
It is for a station whose chatters can upload pictures of their own, or post pictures and documents in the chat ([the switches](#before-you-need-it)); with both off, no picture reaches you and you need none of it.
A picture or document posted in the chat is handled exactly as an avatar picture below: refused or removed with the same reason, kept the same way, exported the same way.
The law it rests on is read in the project's recce 31 (2026-11-10); check it against your own country's rules.

If a child is in danger now, call the police first.

## Before you need it

Do these once, today, so that the first time is not the day you meet the form.

1. **Register to report.** In the UK a station must register with the National Crime Agency (NCA) before its first report. In the US, a provider reports through the National Center for Missing and Exploited Children (NCMEC). Whether a volunteer station qualifies, and whether NCMEC takes a non-US provider, were not settled in the recce: ask both before the first need.
2. **Name the one person who sees it.** Settings > Chat pictures > *Who sees pictures kept as evidence*. Until you choose, it is the station's first admin. Whoever it is will look at something distressing: choose a person who agrees to, and tell them this page exists.
3. **Know that your moderators will see it first.** A picture waits for staff or a moderator to approve it, so whoever works the queue meets whatever a malicious account sends. Tell them what to do (below), and that they may stop.
4. **Say it in your terms.** Ban pornography and sexual imagery of anyone under 18 in the chat's terms, and keep the takedown and report features on.

## When a picture looks like it

The person who sees it, in the Screener's **Needs you** column:

1. **Refuse it, or remove it, with its own reason.** Press `H`, then `C` for *Suspected child sexual abuse: keep as evidence* (or choose it by mouse), then `⏎`. The dialog says what happens before you send.
2. **Do not delete it any other way, and do not copy it.** The reason keeps the picture and does not delete it. The chat keeps, for at least a year (`avatars.evidence_days`):
   - the upload exactly as it arrived (the *original*), if it was refused while it waited (an approved picture keeps only its clean copy), and the picture as the chat shows it;
   - a snapshot of the uploader and your decision: their chat name and account, when they joined, the address the picture came from **if the station keeps addresses** (it keeps none by default), and your note;
   - the moderation record for the account.
   The chatter receives a statement of reasons that says only that it was removed for breaking the station's rules on illegal content, and how to ask for a review: nothing about keeping it or reporting it.
3. **Nobody else can see it.** Not the other staff, not an admin who is not the named person, not the station key, not the chatter. A moderator cannot open it either. Settings > Chat pictures lists it as *kept as evidence*, so staff know it exists and whom to ask.

The named person then:

4. **Opens Settings > Chat pictures**, finds the picture under *Pictures the station is keeping*, and presses **Export evidence**. The file is a ZIP: the original upload, the picture as shown, the uploader's details and the moderation record, with a README that says what is in it and what is not. Looking (**View**) and exporting are each a line in the moderation record with who did it.
5. **Reports it** (next section).

## Whom to report to

From the recce's reading of the sources; the links are the ones it used.

- **UK.** The Online Safety Act's duty to report child sexual exploitation and abuse content to the NCA applies "regardless of service size or the level of CSEA risk" ([section 66](https://www.legislation.gov.uk/ukpga/2023/50/section/66), in force 7 April 2026; [Ofcom's guidance](https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/duty-to-report-child-sexual-exploitation-and-abuse-csea-content-know-the-rules-and-how-to-comply)). *Detected* includes a find by your staff or a report by a user. The route is the NCA's portal, not the Internet Watch Foundation, and you must register first. A service that already reports the same content to NCMEC does not report it to the NCA as well. [The Regulations (SI 2026/268)](https://www.legislation.gov.uk/uksi/2026/268/made/data.htm) set what a report contains and when, and require keeping the detected content and the user's data for one year and the report's reference for five. The chat keeps the first for you; **keep the reference** yourself.
- **US.** After actual knowledge of apparent child pornography, a provider must report it to NCMEC's CyberTipline as soon as reasonably possible. A report counts as a request to preserve the contents for one year ([18 U.S.C. 2258A](https://www.law.cornell.edu/uscode/text/18/2258A)). A failure to report, knowingly and wilfully, carries fines. A chat's host is almost certainly a provider within it (the recce's inference).
- **EU.** The Digital Services Act's Article 18 requires prompt notice to the police of the Member State concerned (or to Europol) of a suspected offence "involving a threat to the life or safety of a person"; its recital names the directive on child sexual abuse. Whether that covers a picture on its own is the recce's inference: ask your national authority. Article 16 notices need not give a name or an email address for these offences.

The station files the report. The chat does not, and does no matching against lists of known images (none applies to a station of this size; the recce found it required by no one here).

## What never to do

- **Never forward, print, copy or screenshot the picture,** to a colleague, a lawyer, a chat, a ticket or an email. Give it only to the reporting channel, through the export.
- **Never delete it first.** The law asks for a report, then keeping. The station can delete it later (below), after the report.
- **Never open it more than you must.** One person looks; the rest of you do not.

## Keeping and deleting

The picture, its original if it has one, and the snapshot stay for at least a year (`avatars.evidence_days`, never below 366, so a leap year is covered), whatever happens to the evidence hold or the account: releasing the hold, erasing the account and the moderation record's own retention do not take it, and the retention sweep deletes it only after the year, and then only if no hold names the account.

If you want it gone sooner, for instance because the NCA has the report and your own advice says so, the person named to see the evidence, or an admin, can delete it in Settings > Chat pictures with **Delete now…**; other staff cannot. For this reason the dialog stops first: it says that the law asks you to report it and then keep it, asks why, and asks you to confirm. The deletion is a line in the moderation record with who, when and why. The station decides; the chat does not.

## What the evidence does not have

- **The uploader's email address.** The chat keeps only a keyed hash of an address, so no address can be read from the database. Your mail provider may hold the address; the authorities can ask.
- **An IP address,** unless the station's retention keeps addresses. It keeps none by default, so most exports have none.

If you need either, say so in the report: the authorities, not the chat, can ask for the rest.

## See also

- [Moderate the chat](/docs/how-to/moderate-the-chat/), for the queue, the keys and the statement of reasons.
- [The chat service](/docs/reference/chat/#avatars), for what is kept and the routes.
- [Choose your avatar in the chat](/docs/how-to/choose-your-avatar/), for what a chatter is told.
