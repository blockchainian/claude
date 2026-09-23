---
name: find-domain-names
description: Brainstorm brand names for a product and return the ones whose domain is actually registrable — short coined words (and metaphor words) distilled from a theme you give, checked live on .xyz/.ai/.fun via Namecheap's official API, with same-name collisions against anything popular filtered out. Use for "find me a domain", "name this product", "brainstorm a brand name and check the domain", "什么域名还能注册". NOT for checking one specific domain you already have (just run scripts/check.mjs), and NOT for logo/visual identity.
---

# Find Domain Names

Brainstorm a brand name and hand back only the ones you can actually register.
The naming taste is fixed (below); the one thing that changes per run is the
**theme**, which you always ask for. Availability is checked live against
Namecheap's official API, so a name on the final list is real, not a guess.

## Setup (once, needs an API key)

The checker calls Namecheap's official `domains.check`, which needs an API key
and a whitelisted IP. Config lives in the shared plugin file
`~/.config/blockchainian/claude.json`, under this skill's key:

```json
{ "find-domain-names": { "namecheap": { "apiUser": "<username>", "apiKey": "<key>" } } }
```

**Before the first run, check that section exists.** `scripts/check.mjs` exits
with a setup message if it's missing. If it is, walk the user through it — do
not brainstorm until it's set:

1. Open Namecheap → Profile → Tools → **API Access**, enable it, copy the **API Key**.
   (API access needs 20+ domains, a $50 balance, or $50 spent in the last 2 years.)
2. On the same page, **whitelist the current IP** (Namecheap requires it; this
   skill does **not** manage the whitelist — the user adds their IP there once).
3. Write `apiUser` (the Namecheap username) and `apiKey` into the config above.

`ClientIp` is auto-detected each run, so a changing IP is fine **as long as
that IP is whitelisted**. An un-whitelisted IP makes the API return an error —
tell the user to whitelist it.

## The flow

1. **Ask the user for the theme / product.** Always. There is no default —
   the theme drives the whole metaphor pool. (One line is enough: "a crypto
   trading app", "a sleep-tracking wearable", "a co-op board game".)
2. **Generate candidates** in the fixed style below, aimed at the sweet spot:
   a **short coined word distilled from a metaphor of the theme** — e.g. for a
   wealth product, *treasure trove → trovy*. Coined words first, metaphor words
   second, and **no compound words**.
3. **Check availability** with `scripts/check.mjs` across `.xyz / .ai / .fun`.
   Keep `available-standard`; carry `available-premium` as flagged backups with
   their price; drop `taken` and (usually) `reserved-premium`.
4. **Collision check by popularity.** WebSearch the survivors. Drop a name only
   when it collides with something **popular / well-known** — a notable crypto
   or tech project, a mainstream brand, a famous title. An obscure namesake is
   not a reason to cut. Judge by how well-known the clash is, not by mere
   existence of a namesake.
5. **Present a ranked shortlist**, coined above metaphor, each with a one-line
   note on **where the meaning is buried** (the point is it's discoverable, see
   the style). Flag any premium picks with their buy/renew price.

The only things that interrupt the user are the theme question (step 1) and the
setup prompt when the key is missing. **Never ask the user to pick a style
direction** — the style is already decided:

## Fixed style

- **Interesting, any register** — playful, witty, or bold all welcome; don't
  self-limit to one vibe.
- **Meaning buried one layer, but not too deep** — don't put the theme word on
  the face; make the reader discover it. But keep it gettable by an ordinary
  person (a name whose sense only appears with etymology is buried too deep —
  cut it).
- **Coined first, metaphor second, no compounds** — compound two-word names run
  long; skip them.
- **Not tied to one product feature** — the name lives at the brand layer, not
  bound to "fast" / "sniping" / "info-edge".

Length caps (a hard filter, not a score): **coined ≤ 6 letters, metaphor ≤ 8.**
Priority is separate and by type: coined outranks metaphor regardless of length.

## The checker — `scripts/check.mjs`

```
node scripts/check.mjs <name|domain> ...
```

A bare word (`trovy`) expands to `.xyz/.ai/.fun`; a full domain (`trovy.xyz`)
is checked as given. Prints one row per domain: `domain  status  price`, where
status is `available-standard` (registrable, normal price), `available-premium`
(registrable but premium — price shown), `reserved-premium` (held by the
registry, price shown), or `taken`. Batches of 50 per API call.

## Tests

```
node --test skills/find-domain-names/scripts/test_check.mjs
```

Covers XML parsing and the four-state classification on a fixed Namecheap
response (no network).
