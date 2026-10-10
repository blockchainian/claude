# Review the users doc adversarially

The doc profiles the accounts that mention an app on X. Find everything wrong, overstated or
unsupported in it. Do NOT rewrite the doc; return findings only.

## Files

Read them all:

- the doc, in Chinese
- the app facts file
- `authors.json`: per-account features and assigned role, keyed by handle
- `role_stats.json`: per role, authors, posts and engagement with their shares (`pa`, `pp`, `pe`),
  plus totals and thresholds
- `reps/<role>.jsonl`: the representative accounts and the posts the motivation read used, one
  `{account, role, features, posts:[{date, eng, lang, text, url, label}]}` per line

## Checks

Do every one:

1. **Citation integrity**: find each quoted post in the reps files by url or handle. Does the quote
   match the real text, and does it support the claim it is attached to? Flag misquotes, sarcasm
   read as praise, and a competitor or own-token shill cited as fandom.
2. **Segment numbers**: do the table's shares match `role_stats.json`? Do the headline percentages
   (one-post share, KOL share of engagement, promoter share) reconcile with the files?
3. **Die-hard table**: spot-check 4–5 rows (n, days, ref_r, inbound) against `authors.json`. Is the
   rule stated in the doc the one applied? Are the official accounts kept out of it?
4. **Motivation claims**: is each bucket's stated motivation supported by that bucket's
   representative posts, or does it overreach from one or two accounts?
5. **Caveat honesty**: the doc says its own classification is a proxy. Is that self-critique
   accurate and sufficient, or does the doc still lean on a number it just called unreliable?
6. **Overclaims**: flag any sentence asserting more than the data supports, such as calling an
   account "paid by the app" when only a referral code is visible, or stating a whole segment's
   motivation as fact from seven samples.

## Output

Write plain text, concise:

- Numbered FINDINGS, most severe first, at most 12. Each gives its severity (MUST-FIX, SHOULD-FIX
  or NIT), the exact claim and its location, what is wrong, and the concrete fix.
- A one-line VERDICT: are the doc's three key conclusions supported?

A cited post that does not support its claim is at least SHOULD-FIX. Do not invent problems; when
something checks out, say so in one line.
