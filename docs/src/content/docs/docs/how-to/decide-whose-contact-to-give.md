---
title: "Decide whose contact to give callers"
description: "A step-by-step chart for a station: whether its callers must hear or read consent words at all, whether those words need a contact, whether that contact must be a named person, and whose name and role to give."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 12
---

:::caution[Not legal advice]
This chart sums up what WeaveSignal's own research found, with the sources it names (checked in October and November 2026).
Rules differ from country to country, change, and depend on what your station is.
Where a step says **check with your own advice**, the research did not confirm it: ask someone who knows the law where you are.
If you are unsure at any step, take the safer answer: keep the words on, and give a contact.
:::

Callers reach the station two ways: the player page's **Call the studio** form, which shows the consent words with a tick box, and, from 1.2, the [phone line](/docs/how-to/take-phone-calls/), which plays them.
Both name a **takedown contact**: where a caller asks for their call to be taken down.
WeaveSignal itself only needs an email address or a link there, never a person's name.
This chart helps you decide whether your station needs more than that, and whose contact it should be.

## The chart

Start at step 1 and follow the arrows.

1. **Do you broadcast on FM, AM or DAB under a licence, or only on the internet?**
   - **Under a licence** (as well as on the internet, or not): your licence and your regulator's code come first.
     In the UK, Ofcom's Broadcasting Code (Section 8) asks you to identify the station, explain what the call is for, and say the call "is being recorded for possible broadcast"; callers to a phone-in are taken to have agreed to be on air (rules 8.12 and 8.6).
     In Canada, the Radio Regulations ask for consent before a call is broadcast, unless the caller rang to take part.
     In the US, the FCC's rule 47 CFR 73.1206 asks a licensee to tell the other party before recording a call for broadcast, and presumes it for a caller to a station that usually broadcasts calls.
     In Australia, the Commercial Radio Code asks for consent before a call is broadcast.
     → Keep the consent words **on**, on the player page and the phone line. Go to step 4.
   - **Only on the internet**: go to step 2.
2. **Do you record or keep calls?**
   This means anything kept after the show: the archive, a podcast, clips, or a [retention setting](/docs/how-to/set-retention/) other than No retention.
   - **Yes**: you must be able to show that the caller agreed while the recording exists, in the UK and the EU (GDPR Article 7(1)).
     In Germany, recording someone's private words without the right to is a crime (StGB § 201), and the screener's talk with a caller before they are on air is the part most at risk.
     → Keep the consent words **on**. Go to step 4.
   - **No**: go to step 3.
3. **Is the number (or the call page) only ever given out on air, as a live phone-in?**
   - **Yes**: ringing it may itself count as agreeing to be on air, and nothing is kept. The rules the research found that say so (Ofcom's rule 8.6, Canada's Radio Regulations) are written for licensed broadcasters; whether the same holds for an internet-only station where you are, **check with your own advice**.
     → You may turn the **phone line's** words off (`consent_words = false`, or `PHONE_CONSENT_WORDS=false` on Compose and the Quadlets): callers go straight into the queue, and the station's records say no words were played.
     The player page still shows its words and asks for a contact before call-in opens, whatever you choose here.
     Some US states ask for every party's consent before a call is recorded, and the rules in Australia's states differ: if your callers are there, **check with your own advice**.
     Go to step 4.
   - **No**, or you are not sure: keep the words **on**. Go to step 4.
4. **Whose contact do the words give?**
   - **The station's own** (the takedown contact on the [setup page](/docs/how-to/set-up-the-station/), or `CONSENT_CONTACT`): always, and it is the one the phone line's recorded words name, so a recording does not go out of date at every change of show.
   - **A show's own as well**: the player page's words name the show on air's contact first and the station's as the host's ([Set up your show](/docs/how-to/set-up-your-show/)). Give a show its own when its presenter, not the station, answers for its callers' recordings.
   - Go to step 5.
5. **Does the contact need to be a named person, or will a role do?**
   For WeaveSignal, a role is enough: an inbox someone reads, such as `calls@station.example`, or a link to a form.
   It is better than a person's own address, because it outlives a change of staff.
   A person may still need to be named elsewhere:
   - **A licensed broadcaster**: your licence names the licensee, and your regulator may ask for a person responsible for compliance. **Check with your own advice**: the research did not cover it.
   - **The station has the live chat on, with UK listeners**: Ofcom's codes for services where users talk to each other include a named person accountable for online safety (reported in WeaveSignal's research, not confirmed). That person is someone in your organisation; the codes as reported do not say their name must be shown to callers. **Check with your own advice.**
   - **The station is in the EU, or serves people there, with the chat or uploads on**: the EU's Digital Services Act asks every hosting service, however small, for notice and action, statements of reasons and reporting threats to life (Articles 16 to 18). Whether it also asks you to publish a point of contact was not checked. **Check with your own advice.**
   - **Data protection (UK and EU)**: people whose calls you keep are owed an account of who keeps their data and how to reach you. A station's or organisation's name and a role address usually serve; **check with your own advice**.
   - **None of these**: no named person is needed. Go to step 6.
6. **If you name a person, which one?**
   Name the person who can act on a takedown within your retention period, by the role they hold: the station manager, or the presenter for their own show.
   Give their role with their name ("Shona MacLeod, station manager"), so a caller knows who answers, and change it when the role changes hands.
   For the chat's accountable person, name the person who decides how the chat is moderated.

## What WeaveSignal does with each answer

- **The player page's consent words** need a contact while they name `{contact}`: call-in stays closed until the station sets one (ADR-0124). The default words do.
- **The phone line's words** can be turned off (`phone.consent_words`); the help beside the switch in the studio says when, in the words of [Take phone calls](/docs/how-to/take-phone-calls/#the-consent-words).
- **Each call's consent record** keeps the words and the contact exactly as the caller was given them, so a later change does not rewrite what a caller agreed to.

## Where this comes from

- Ofcom Broadcasting Code Section 8, rules 8.6 and 8.12; GDPR Article 7(1); StGB § 201(1); 47 CFR 73.1206; Canada's Radio Regulations, 1986, s. 3(e); Australia's Commercial Radio Code 2026, cl. 6.1(b): WeaveSignal's research on keeping records by jurisdiction (recce 27, section 4, checked 2026-10-25).
- The UK Online Safety Act's codes and the EU's Digital Services Act: WeaveSignal's research on the live chat's safety (recce 20, checked 2026-10-23); the accountable person is *reported* there, Articles 16 to 18 *verified*.
- US states' all-party consent and Australia's state laws are *reported* in recce 27, not confirmed.
