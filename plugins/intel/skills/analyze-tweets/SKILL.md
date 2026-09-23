---
name: analyze-tweets
description: Turn a fetched X/Twitter mentions archive (tweets.jsonl from fetch-x-mentions) into a concise, data-driven Chinese reception doc plus charts — hot topics with a dated timeline, what people like, what people dislike, each ranked by frequency over every post and backed by id-verified quotes. Use when asked to analyze/分析 what X is saying about an app, brand or protocol from an existing mentions dataset. NOT for fetching the tweets (the archive must already exist) and NOT for App Store reviews (use analyze-appstore-reviews).
---

# Analyze Tweets

One app's X mentions → an evidence-only reception doc: what the data is, what users
like and dislike about the app, what they ask for, the timeline, and what it means for
us. Every number comes from the JSON; every post is read and labeled by a subagent (no
sampling, no keyword filter, no clustering model: those drop the concrete content);
every quote is verbatim and id-verified. Concrete beats short: name the feature, the
bug, the number, the date.

## Input & output

- **Input**: `mentions/<slug>/tweets.jsonl` as written by `fetch-x-mentions.mjs`
  (one tweet per line with `id, author, text, created_at, likes, replies, lang, url`;
  `clean.mjs` dedups by id). Run
  on a finished archive; pass `--since/--until` to analyze a window of it. The doc
  title states the window.
- **Output**, next to the input: `mentions/<slug>/reception.md` + `images/`;
  `mentions/<slug>/labels.jsonl`, one line per post (`id, about, sentiment, topic,
  feature, point, request, interest`), committed, so the next run labels only the
  posts it has not seen and any window can be reported from the store without
  agents; and one shared `mentions/vocab.json` (`{topics, features, interests}`)
  for every app, the vocabulary seen so far, which seeds the next run's prompt
  and grows after every run on any app (the apps overlap heavily; one list stays
  maintained, one per app goes stale). A windowed run writes
  `reception-<from>..<to>.md` with the shared date prefix written once
  (`reception-2026-09-01..22.md`, `reception-2026-08-15..09-22.md`,
  `reception-2024-01-01..2028-01-20.md`) so it never overwrites the all-time doc.

`S="${CLAUDE_PLUGIN_ROOT}/skills/analyze-tweets/scripts"` below.

## Runs

The store makes every run incremental. Three ways to invoke this skill:

- **First run on an app**: no `labels.jsonl` yet; every clean post is chunked and
  labeled; the all-time `reception.md` is written.
- **New data arrived** ("analyze this week's <app> tweets"): run steps 1–4 on the
  whole archive; chunk.mjs skips every post already in `labels.jsonl`, so only the
  new posts go to labelers; then aggregate with `--since/--until` for the window
  and write `reception-<from>..<to>.md`, or without a window to refresh
  `reception.md`.
- **Report only** ("report on August from what we have"): steps 1, 5, 6 only; no
  labelers, everything comes from the store.

## Procedure

### 1. Clean (script)

```
node $S/clean.mjs <tweets.jsonl> --out <scratch>/clean.json [--since YYYY-MM-DD] [--until YYYY-MM-DD]
```

Drops bot alert templates (`Route:`, `MIGRATION`, `CTO SIGNAL`, `WALLET FLOW CHECK`,
`Quick Buy`, `dm us`; extend with `--bot-pattern`), posts tagging ≥ 6 handles,
duplicates after stripping handles/urls, and texts under 8 chars. Prints raw/clean
counts, date range, account count, top authors, clean count by day, and what was
dropped. The raw→clean numbers become the doc's scope line.

### 2. Timeline (script)

```
node $S/aggregate.mjs <clean.json> <scratch> --timeline [--since D] [--until D]
```

Prints the top 3 posts by likes for every day of the window — the raw material
of the timeline section. No keyword list: what users talk about comes from the
labels in step 3.

### 3. Label the posts that have no label yet (subagents)

```
node $S/chunk.mjs <clean.json> --size 2000 --out <scratch> --labels mentions/<slug>/labels.jsonl [--since D] [--until D]
```

Only posts missing from `labels.jsonl` (and inside the window, if given) are
chunked; each chunk is written as
`chunkN.json` and `chunkN.tsv` (one post per line: id, author, likes, date, lang,
text). Spawn `general-purpose` Sonnet labelers, one chunk each, about 10 in flight;
start the next when one finishes. Write the prompt once to `<scratch>/PROMPT.md` and
point each labeler at it. The prompt names the app's official handles (current and
former), founder and team, products and features, and competitors, and says:

- First print all the posts, in batches of 100 as `id \t author \t likes \t date
  \t lang \t text`, into batch files under a private `work_N/` directory (never a
  shared one: labelers run side by side), then Read every batch file (the Read
  tool returns at most 25k tokens per call, Bash output about 30 KB, so reading
  is batched whatever the prompt says). After each batch, write that batch's
  labels as DATA — a file with one object per post id, the label the labeler
  decided while reading — never a classifier: no keyword rules, no regex, no
  function that derives labels from text. A final script only concatenates the
  batch data files, checks ids and order, and writes `labelsN.json`. (A prompt
  that asked for "one script holding all labels" made every labeler compress
  its judgments into regex rules, which is the keyword whitelist again; the
  per-batch data files are what keeps the labels per post.)
- Before merging, audit each `work_N/`: a `.py` there with `re.compile`,
  `re.search` or `in text`-style rules, or batch files whose ids add up to fewer
  than the chunk's posts, means that chunk was classified by rules — discard it
  and rerun that chunk.
- Then two files:
  - `labelsN.json`: one object per post, chunk order, every id present:
    `{id, about, sentiment, topic, feature, point, request, interest}`.
    - `about`: true only if the post is about the app itself (a reply that only
      carries the app's handle from the thread and talks about something else is
      false).
    - `sentiment`: for about=true, like / dislike / neutral / noise (noise: about
      the app but content-free, e.g. "gm @app"); for about=false, `irrelevant`
      (talks about something else: another token, a person in the thread) or
      `noise` (content-free: one-word replies, emoji, giveaway begging, bot
      alerts). Irrelevant and noise are reported separately in the data section.
    - `topic`: what the post is about as a subject people discuss (the event, the
      company, the ecosystem, the culture). The prompt lists the topics in
      `vocab.json` as examples (first run: product-features, outages-execution,
      fees-pricing, insiders-manipulation, scams-rugs, verification-listing,
      competition, funding-revenue-growth, chain-integrations, kols-celebrities,
      challenges-giveaways, token-communities, security-custody,
      regulation-regions, culture-memes, support-team) and says: if none fits,
      write your own. Never an `other` bucket.
    - `feature`: which part of the app the post is about; the prompt lists the
      features in `vocab.json` as examples (first run: fees, cross-chain-balance,
      copy-trading, leaderboard, thesis, clans, callouts, verification,
      limit-orders, mobile-app, web-app, ui, stability, custody-wallet, support,
      referral, apple-pay-onboarding, livestream, followers-social,
      chain-integration), and if none fits, write your own; `none` when no part of
      the app applies.
    - A topic or feature the labeler writes itself must be a 2–4 word English
      noun phrase, lowercase with hyphens, specific enough to tell apart from the
      examples, and reused for every post about the same thing (check the
      examples first, then your own earlier names).
    - `point`: ≤ 12 words saying what the post claims (the bug, the number, the
      complaint), never the feature name alone.
    - `request`: ≤ 12 words when the post asks to add, fix, change or remove
      something, else null.
    - `interest`: null when the speaker has no stake; otherwise the kind of
      stake, from the interests in `vocab.json` (first run: referral — posts a
      code or link; creator-rewards — earns callout / thesis rewards; token-team —
      promotes their own token; official-partner — the company, staff, partners;
      paid-promotion); write your own if none fits.
    Non-English posts are labeled like the rest, text fields in English. Topic ×
    sentiment gives the hot-topics section; feature × sentiment gives likes,
    dislikes and requests; `interest` gives the interested-party share.
  - `summaryN.txt` (plain text, not `.md`: the harness refuses subagent
    report-style markdown): sections LIKES, DISLIKES, REQUESTS with the top 5 points
    by post count, each with ids and one verbatim quote ≤ 25 words (Chinese posts
    quoted in Chinese, never spanning a t.co link), then FACTS: numbers, launches,
    outages, funding, partnerships, store removals, each with its id.
- Reply with one line: `chunk N: <posts> labeled, <noise%> noise, <about%> about`.

A 2000-post chunk is about 110k tokens of posts in and 70k of labels out, but a
labeler runs 40–70 tool turns and re-sends its context each turn: measured about
15M input tokens (roughly $5 at Sonnet prices, mostly cache reads) per 2000-post
chunk. Sonnet's window is 1M and its output cap 128k, so do not go above ~3000
posts per chunk; below 2000 the fixed 59k-token setup per agent dominates.

Then fold the chunks into the store and grow the vocabulary:

```
node $S/merge-labels.mjs mentions/<slug>/labels.jsonl <scratch> --vocab mentions/vocab.json [--rename topic:old=new ...]
```

It folds the chunk files into `labels.jsonl` by id (a relabeled id overwrites),
applies the aliases already in `vocab.json`, adds every new topic / feature /
interest value that reaches 1% of this batch's about=true posts to `vocab.json`,
and prints the values below that line with their counts. Read the printed lists:
a new value that means the same as an existing one is folded with
`--rename topic:old=new` (field-scoped; the rename rewrites the store and is kept
as an alias so later runs fold it automatically); the rest stay in the labels but
not in the vocabulary. Commit `vocab.json` with the report: the next run, on any
app, seeds its prompt from it.

### 4. Aggregate and verify (script)

```
node $S/aggregate.mjs <clean.json> <scratch> --labels mentions/<slug>/labels.jsonl [--since D] [--until D] [--top 300]
```

Prints how many window posts have a label (re-run any chunk whose noise share is
far below the others: a labeler that marks one-line reply banter as neutral instead
of noise inflates "about" counts); topics with like / dislike / neutral counts and
the peak month of each; sentiment over all posts and over the top-liked
(volume share vs attention share); like, dislike and request counts per feature with
unique authors; the most frequent `point` and `request` texts; the interested-party
share of likes by kind of `interest`;
and, for every summary, unknown ids and quotes that are not a substring of any
post. Rank likes, dislikes and requests by their label counts; use the summaries
only to pick quotes and facts. Take the final quote text by id from `clean.json`,
never from a labeler's paraphrase, and link it as
`https://x.com/<author>/status/<id>`.

### 5. Charts (script)

```
echo '{"out_dir":"<slug>/images","charts":[
  {"type":"bar","file":"<slug>-hot-topics.png","title":"热点话题（提及条数）","labels":[...],"values":[...],"color":"#2a78d6"},
  {"type":"daily","file":"<slug>-daily-volume.png","title":"每日提及量与当天事件","days":["09-02",...],"values":[...],"events":{"09-10":"App Store 下架"}},
  {"type":"bar","file":"<slug>-likes.png","title":"用户喜欢 App 的什么","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<slug>-dislikes.png","title":"用户不满 App 的什么","labels":[...],"values":[...],"color":"#eb6834"},
  {"type":"bar","file":"<slug>-requests.png","title":"用户想要什么","labels":[...],"values":[...],"color":"#8a5cd6"}
]}' | $S/render_charts.py /dev/stdin
```

Chinese labels, transparent background, dual-mode gray ink, title only. Event
labels on the daily chart sit horizontally above their day, staggered on three
levels with a leader line, so keep each event name short (about 8 characters) and
mark at most ten days. `$` in a label is matplotlib mathtext; escape it. Open each
PNG and check: no label collisions, headroom above the tallest bar.

### 6. Write `reception.md` (Chinese, concrete, evidence only)

```
# <App> 推特口碑（<start> → <end>）

## 一、这批数据是什么
- what the app is, in two lines, as the posts describe it (product, chains, launch dates seen in the data)
- days, raw count, accounts; clean count after dropping bots/mass-tags/dupes
- composition over all posts: noise / shilling other tokens / referral / real praise / real complaints, in %
- the volume story: when it jumped and why (one line each)
- caveats about the search terms (other products sharing the name, gaps)
![](images/<slug>-daily-volume.png)

## 二、热点在哪儿
![](images/<slug>-hot-topics.png)
1. <topic>: <count> 条, 好评 <n> / 差评 <n>, 峰值 <month>, <what the peak was about>
   > @handle：[verbatim text](url)

## 三、用户喜欢 App 的什么
![](images/<slug>-likes.png)
1. <feature or behavior>: <what exactly they praise>, <count> 条 / <authors> 人（say when the speakers are interested parties）
   > @handle：[verbatim text](url)

## 四、用户不满 App 的什么
![](images/<slug>-dislikes.png)
1. <feature or behavior>: <the specific bug, fee, delay, rule>, with the numbers users cite
   > @handle：[verbatim text](url)

## 五、用户想要什么
![](images/<slug>-requests.png)
1. <request>: add / fix / change what, <count> 条
   > @handle：[verbatim text](url)

## 六、时间线：声量、情绪、关键事件
- MM-DD event, one line, with the number it moved
  > @handle：[verbatim text](url)
- how like/dislike share moved over the window (by month or quarter)
- facts worth keeping: funding, users, revenue, integrations, outages, store removals (id-linked)

## 七、对我们的启示
- one line per lesson, each tied to a finding above: what to copy, what to avoid, what users will ask us for

## TL;DR
- at most 5 bullets, plain words
```

Rules:
- No 方法 / 可信度 sections, no process narration, no agent or model talk.
- Quote line is exactly `> @handle：[text](url)`: link on the text, handle plain,
  no like counts or any number next to the handle, no italics. Every quote line
  carries text. Chinese posts are quoted in Chinese.
- Every claim names the feature, the bug, the number or the date. "故障 bug" or
  "骂战" is not a finding; "买入后 12 小时无法卖出，09-08 当天 41 条" is.
- Charts sit at the top of their section.

### 7. Ship

`git add` `reception.md`, `images/` and `labels.jsonl` only, commit, push.
`clean.json`, the chunks, `labelsN.json` and `summaryN.txt` are scratch.

## Requirements

- Node ≥ 20 for the `.mjs` scripts; `uv` for `render_charts.py` (matplotlib);
  CJK font at `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Tests

```
node --test skills/analyze-tweets/scripts/test_analyze_tweets.mjs
```
