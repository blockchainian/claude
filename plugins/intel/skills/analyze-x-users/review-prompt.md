# Task: adversarial review of a user-segmentation and motivation doc

A colleague profiled the accounts that mention an app on X. Find everything wrong, overstated or unsupported. Do NOT rewrite the doc; return findings only.

## Files (paths in your prompt; read them all)
- The doc (Chinese).
- The app facts file.
- `authors.json`: per-account features and assigned role (keys are handles).
- `role_stats.json`: per role, authors / posts / engagement and their shares (`pa`, `pp`, `pe`), totals, thresholds.
- `reps/<role>.jsonl`: the representative accounts and the posts each motivation agent read; each line = `{account, role, features, posts:[{date, eng, lang, text, url, label}]}`.

## Checks (do every one)
1. **Citation integrity** — for each quoted post (find it in the reps files by url or handle): does the quote match the real text, and does it support the claim it is attached to? Flag misquotes, sarcasm read as praise, a competitor or own-token shill cited as fandom.
2. **Segment numbers** — do the table's shares match `role_stats.json`? Do the headline percentages (one-post share, KOL share of engagement, promoter share) reconcile with the files?
3. **Die-hard table** — spot-check 4–5 rows (n, days, ref_r, inbound) against `authors.json`. Is the rule stated in the doc the one applied? Are the official accounts kept out of it?
4. **Motivation claims** — is each bucket's stated motivation supported by that bucket's representative posts, or is it overreaching from one or two accounts?
5. **Caveat honesty** — the doc says its own classification is a proxy. Is that self-critique accurate and sufficient, or does the doc still lean on a number it just called unreliable?
6. **Overclaims** — any sentence asserting more than the data supports (calling an account "paid by the app" when only a referral code is visible; stating motivation as fact for a whole segment from seven samples).

## Return (concise, plain text)
- Numbered FINDINGS, most severe first, at most 12. Each: severity (MUST-FIX / SHOULD-FIX / NIT), the exact claim and its location, what is wrong, the concrete fix.
- One-line VERDICT: are the doc's three key conclusions supported?
- A cited post that does not support its claim is at least SHOULD-FIX. Do not invent problems; if something checks out, say so in one line.
