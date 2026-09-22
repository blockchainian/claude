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

### 3. Label every post (subagents)

```
node $S/chunk.mjs <clean.json> --size 300 --out <scratch>
```

Every clean post lands in one `chunkN.json`. Spawn `general-purpose` Sonnet
labelers, one chunk each, 8 per message; start the next wave when one finishes. The
prompt names the app's official handles (current and former), products and
competitors, and asks for two files per chunk:

- `labelsN.json`: for every post `{id, about, sentiment, feature, point, request}` —
  `about` true only if the post is about the app itself; `sentiment` like /
  dislike / neutral / noise; `feature` the app feature or behavior the post is
  about, from the list the prompt gives (fees, cross-chain balance, copy trading,
  leaderboard, token verification, limit orders, mobile app, web app, streaming,
  callouts / creator rewards, airdrop, support, UI, stability, custody, referral;
  plus free additions, `competitor` for comparisons, `none` when not about the app);
  `point` ≤ 12 words saying what the post claims (the bug, the number, the
  complaint), never the feature name alone; `request` ≤ 12 words when the post asks
  to add, fix, change or remove something, else null. Chinese and other non-English
  posts are labeled like the rest.
- `summaryN.txt` (plain text, not `.md`: the harness refuses subagent report-style
  markdown): top 5 likes, top 5 dislikes, top 5 requests, each with post ids and a
  verbatim quote ≤ 25 words, then any facts worth the timeline: numbers, launches,
  outages, funding, partnerships, with ids. If the write is still refused, the
  labeler returns the summary in its reply; save it yourself.

### 4. Aggregate and verify (script)

```
node $S/aggregate.mjs <clean.json> <scratch> [--top 300]
```

Prints how many chunks came back (re-run any chunk whose noise share is far below
the others: a labeler that marks one-line reply banter as neutral instead of noise
inflates "about" counts); sentiment over all posts and over the top-liked
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

`git add` `reception.md` and `images/` only, commit, push.

## Requirements

- Node ≥ 20 for the `.mjs` scripts; `uv` for `render_charts.py` (matplotlib);
  CJK font at `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Tests

```
node --test skills/analyze-tweets/scripts/test_analyze_tweets.mjs
```
