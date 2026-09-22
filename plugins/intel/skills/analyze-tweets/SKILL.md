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
- **Output**, next to the input: `mentions/<slug>/reception.md` + `images/`.

`S="${CLAUDE_PLUGIN_ROOT}/skills/analyze-tweets/scripts"` below.

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

### 2. Timeline and known-topic counts (script)

```
node $S/topics.mjs <clean.json> --topics <scratch>/topics.json
```

Prints the top 3 posts by likes for every day — the raw material of the timeline
section — and, for `topics.json` (`{"<中文标签>": "<regex>"}`: fees, airdrop, each
competitor, App Store, the app's own feature names), hits / unique authors / likes /
peak day. The regex counts only what is listed; discovery of what users actually
talk about comes from step 3, never from this list.

### 3. Label the posts that have no label yet (subagents)

```
node $S/chunk.mjs <clean.json> --size 2000 --out <scratch> --labels mentions/<slug>/labels.jsonl
```

Only posts missing from `labels.jsonl` are chunked; each chunk is written as
`chunkN.json` and `chunkN.tsv` (one post per line: id, author, likes, date, lang,
text). Spawn `general-purpose` Sonnet labelers, one chunk each, about 10 in flight;
start the next when one finishes. Write the prompt once to `<scratch>/PROMPT.md` and
point each labeler at it. The prompt names the app's official handles (current and
former), founder and team, products and features, and competitors, and says:

- First print all the posts, in batches of 100 as `id \t author \t likes \t date
  \t lang \t text`, into batch files under a private `work_N/` directory (never a
  shared one: labelers run side by side), then Read every batch file in a row
  (the Read tool returns at most 25k tokens per call, Bash output about 30 KB, so
  reading is batched whatever the prompt says). Do not write labels between
  batches: read all batches first, then write ONE Python script holding the
  labels of all posts and run it once to produce `labelsN.json` and check ids and
  order. A labeler that wrote a labels file per batch and stitched them cost 2.7×
  the one that built the whole array in one script (172 vs 74 turns).
- Then two files:
  - `labelsN.json`: one object per post, chunk order, every id present:
    `{id, about, sentiment, feature, point, request}` — `about` true only if the
    post is about the app itself; `sentiment` like / dislike / neutral / noise
    (strict: a reply that says nothing about the app is noise); `feature` the app
    feature or behavior the post is about, from the list the prompt gives (fees,
    cross-chain balance, copy trading, leaderboard, token verification, limit
    orders, mobile app, web app, streaming, callouts / creator rewards, airdrop,
    support, UI, stability, custody, referral; plus free additions, `competitor` for
    comparisons, `none` when not about the app); `point` ≤ 12 words saying what the
    post claims (the bug, the number, the complaint), never the feature name alone;
    `request` ≤ 12 words when the post asks to add, fix, change or remove
    something, else null. Non-English posts are labeled like the rest, point and
    request in English.
  - `summaryN.txt` (plain text, not `.md`: the harness refuses subagent
    report-style markdown): sections LIKES, DISLIKES, REQUESTS with the top 5 points
    by post count, each with ids and one verbatim quote ≤ 25 words (Chinese posts
    quoted in Chinese, never spanning a t.co link), then FACTS: numbers, launches,
    outages, funding, partnerships, store removals, each with its id.
- Reply with one line: `chunk N: <posts> labeled, <noise%> noise, <about%> about`.

A 2000-post chunk is about 110k tokens of posts in and 70k of labels out, but a
labeler runs 40–70 tool turns and re-sends its context each turn: measured 15M
input tokens (about 2M billed-equivalent after cache reads) per 2000-post chunk.
Sonnet's window is 1M and its output cap 128k, so do not go above ~3000 posts per
chunk; below 2000 the fixed 59k-token setup per agent dominates.

Then fold the chunks into the store:

```
node $S/merge-labels.mjs mentions/<slug>/labels.jsonl <scratch>
```

### 4. Aggregate and verify (script)

```
node $S/aggregate.mjs <clean.json> <scratch> --labels mentions/<slug>/labels.jsonl [--since D] [--until D] [--top 300]
```

Prints how many window posts have a label (re-run any chunk whose noise share is
far below the others: a labeler that marks one-line reply banter as neutral instead
of noise inflates "about" counts); sentiment over all posts and over the top-liked
(volume share vs attention share); like, dislike and request counts per feature with
unique authors; the most frequent `point` and `request` texts; the interested-party
share of likes (reward earners, token promoters, official and partner accounts);
and, for every summary, unknown ids and quotes that are not a substring of any
post. Rank likes, dislikes and requests by their label counts; use the summaries
only to pick quotes and facts. Take the final quote text by id from `clean.json`,
never from a labeler's paraphrase, and link it as
`https://x.com/<author>/status/<id>`.

### 5. Charts (script)

```
echo '{"out_dir":"<slug>/images","charts":[
  {"type":"daily","file":"<slug>-daily-volume.png","title":"每日提及量与当天事件","days":["09-02",...],"values":[...],"events":{"09-10":"App Store 下架"}},
  {"type":"bar","file":"<slug>-likes.png","title":"用户喜欢 App 的什么","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<slug>-dislikes.png","title":"用户不满 App 的什么","labels":[...],"values":[...],"color":"#eb6834"},
  {"type":"bar","file":"<slug>-requests.png","title":"用户想要什么","labels":[...],"values":[...],"color":"#8a5cd6"}
]}' | $S/render_charts.py /dev/stdin
```

Chinese labels, transparent background, dual-mode gray ink, title only. Open each
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

## 二、用户喜欢 App 的什么
![](images/<slug>-likes.png)
1. <feature or behavior>: <what exactly they praise>, <count> 条 / <authors> 人（say when the speakers are interested parties）
   > @handle：[verbatim text](url)

## 三、用户不满 App 的什么
![](images/<slug>-dislikes.png)
1. <feature or behavior>: <the specific bug, fee, delay, rule>, with the numbers users cite
   > @handle：[verbatim text](url)

## 四、用户想要什么
![](images/<slug>-requests.png)
1. <request>: add / fix / change what, <count> 条
   > @handle：[verbatim text](url)

## 五、时间线：声量、情绪、关键事件
- MM-DD event, one line, with the number it moved
  > @handle：[verbatim text](url)
- how like/dislike share moved over the window (by month or quarter)
- facts worth keeping: funding, users, revenue, integrations, outages, store removals (id-linked)

## 六、对我们的启示
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
