---
name: find-domain-names
description: Brainstorm brand names for a product and return the ones whose domain is actually registrable — short coined words (and metaphor words) distilled from a theme you give, checked live on .xyz/.ai/.fun via Namecheap's official API, with same-name collisions against anything popular filtered out. Use for "find me a domain", "name this product", "brainstorm a brand name and check the domain", "什么域名还能注册". NOT for checking one specific domain you already have (just run scripts/check.mjs), and NOT for logo/visual identity.
---

# Find domain names

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
checker imports from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env` and fill in only the values the
skills you use need. The checker reads this file directly and ignores exported shell variables.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `NAMECHEAP_USERNAME` | Namecheap account username | Yes | `~/.config/intel/.env` |
| `NAMECHEAP_API_KEY` | Namecheap API key | Yes | `~/.config/intel/.env` |

## Setup

The checker needs Namecheap API access with the caller's public IP whitelisted. If it reports a
missing credential or a rejected IP, have that fixed before checking any names, and do not resend
the same failed request.

## Ask for the theme

Always ask the user for the theme or product first; there is no default, because the theme drives
the whole metaphor pool. One line is enough, such as "a crypto trading app", "a sleep-tracking
wearable" or "a co-op board game". This question and the setup prompt when the key is missing are
the only times to interrupt the user. Never ask them to pick a style direction: the style below is
already decided.

## Generate candidates

Aim for the sweet spot, a short coined word distilled from a metaphor of the theme: for a wealth
product, *treasure trove* becomes *trovy*. Generate coined words first and metaphor words second,
and no compound words, since two-word names run long. Every candidate follows this fixed style:

- **Interesting, in any register.** Playful, witty and bold are all welcome; don't limit yourself
  to one vibe.
- **Meaning buried one layer, but not too deep.** Keep the theme word off the face of the name so
  the reader discovers it, yet keep it gettable by an ordinary person. A name whose sense only
  appears through etymology is buried too deep; cut it.
- **Not tied to one product feature.** The name lives at the brand layer, not bound to "fast",
  "sniping" or "info-edge".

Length caps are a hard filter, not a score: coined words at most 6 letters, metaphor words at most
8. Priority is by type, separately from length: a coined word outranks a metaphor word whatever
their lengths.

## Check availability

Run the candidates through the checker across `.xyz`, `.ai` and `.fun`. Keep `available-standard`
names, carry `available-premium` names as flagged backups with their price, and drop `taken` and,
usually, `reserved-premium`.

## Filter collisions by popularity

WebSearch the survivors and drop a name only when it collides with something popular or well
known: a notable crypto or tech project, a mainstream brand, a famous title. Judge by how well
known the clash is; an obscure namesake is not a reason to cut.

## Present the shortlist

Hand back a ranked shortlist, coined words above metaphor words, each with a one-line note on where
the meaning is buried, since the point is that it can be discovered. Flag every premium pick with
its buy and renewal price.

## Checker

```sh
"$SKILL_DIR/scripts/check.mjs" <name|domain> ...
```

A bare word such as `trovy` expands to `.xyz`, `.ai` and `.fun`; a full domain such as `trovy.xyz`
is checked as given. The checker prints one row per domain, `domain  status  price`, with one of
four statuses:

- `available-standard`: registrable at the normal price.
- `available-premium`: registrable but premium; the price is shown.
- `reserved-premium`: held by the registry; the price is shown.
- `taken`: registered.

It sends up to 50 domains per API call.

## Tests

```sh
node --test skills/find-domain-names/tests/check.test.mjs
```

The tests cover XML parsing and the four-status classification on a fixed Namecheap response,
without network access.
