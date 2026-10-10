# Infer one segment's motivation

Read the app facts file first: what the app is, its official handles, its referral and rewards
mechanics, and the noise common in its mentions.

You work on ONE behavioral segment (bucket). Read its representative accounts' posts and explain
**why these people use or promote the app**, their real motivation, grounded in what they actually
wrote.

## Input

The segment's JSONL file holds one account per line:

`{account, role, features:{n, days, span, avg_eng, inbound, about_r, like_r, dislike_r, ref_r, giveaway_r, trading_r, token_r, tags}, posts:[{date, eng, lang, text, url, label:{sentiment, topic, interest}}]}`

- `n`: posts mentioning the app
- `days`: distinct days active
- `inbound`: distinct other accounts that @-mention this account, a reach proxy
- `ref_r`: share of posts labeled as carrying a referral or paid promotion
- `token_r`: share promoting their own token
- `about_r`: share actually about the app
- `posts`: up to 35 posts, the 20 most engaged plus 15 spread over the account's timeline

## Output

Produce this, concise and structured:

1. **Bucket motivation (headline)**: the 1–3 core motivations that drive THIS segment to use or
   push the app, ranked, each with one line and 2–3 quoted snippets with `@account` and the url.
   Ground every claim in the posts.
2. **Money and affiliation check**: is there evidence these accounts are financially interested or
   paid? Look for referral codes or links, "10% off", "trade with me", clan or team ownership,
   "partner / sponsored / ambassador", giving away money to build a downline, and promoting their
   own token. State how many of the accounts show it, with examples.
3. **What they do when NOT talking about the app**: what else the posts are about, such as trading
   calls, content, giveaways, banter, other products or competitors.
4. **Per-account one-liner**: `@account (n, inbound) — <who they are + motivation>` for each
   account.
5. **Label fit**: does the assigned bucket fit these accounts? Name the misclassified ones and what
   they really are.

Quote only real text from the file, at most 25 words per quote, citing account and url. Be
skeptical: enthusiasm tied to a referral code is a business, not fandom. Do not invent motivations
the posts do not support. Write plain text, without markdown headers.
