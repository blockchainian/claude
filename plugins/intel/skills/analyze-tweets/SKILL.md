---
name: analyze-tweets
description: Turn a fetched X/Twitter mentions archive (tweets.json from fetch-x-mentions) into a concise, data-driven Chinese reception doc plus charts — hot topics with a dated timeline, what people like, what people dislike, each ranked by frequency over every post and backed by id-verified quotes. Use when asked to analyze/分析 what X is saying about an app, brand or protocol from an existing mentions dataset. NOT for fetching the tweets (the JSON must already exist) and NOT for App Store reviews (use analyze-appstore-reviews).
---

# Analyze Tweets

One app's X mentions → a short, evidence-only reception doc: 热点、最喜欢、最讨厌.
Every number comes from the JSON; every post is labeled (no sampling); every quote is
verbatim and id-verified. The doc reads in one pass: result only, no method or process
narration.

## Input & output

- **Input**: `mentions/<slug>/tweets.json` as written by `fetch-x-mentions.mjs`
  (`tweets[]` with `id, author, text, created_at, likes, replies, lang, url`). Run
  on a finished archive; pass `--since/--until` to analyze a window of it. The doc
  title states the window.
- **Output**, next to the input: `mentions/<slug>/reception.md` + `images/`.

`S="${CLAUDE_PLUGIN_ROOT}/skills/analyze-tweets/scripts"` below.

## Procedure

### 1. Clean (script)

```
node $S/clean.mjs <tweets.json> --out <scratch>/clean.json [--since YYYY-MM-DD] [--until YYYY-MM-DD]
```

Drops bot alert templates (`Route:`, `MIGRATION`, `CTO SIGNAL`, `WALLET FLOW CHECK`,
`Quick Buy`, `dm us`; extend with `--bot-pattern`), posts tagging ≥ 6 handles,
duplicates after stripping handles/urls, and texts under 8 chars. Prints raw/clean
counts, date range, account count, top authors, clean count by day, and what was
dropped. The raw→clean numbers become the doc's scope line.

### 2. Hot topics on the whole clean set (script + judgment)

```
node $S/topics.mjs <clean.json> --topics <scratch>/topics.json
```

Write `topics.json` as `{"<中文标签>": "<regex>"}`: start from the generic set
(手续费, 奖励, 空投, rug/捆绑/机器人, each named competitor, App Store, 直播, 慈善,
"it's over") and add the app's own feature names once the timeline shows them. A
handle that is merely tagged in replies is not a topic. The script prints per topic
hits / unique authors / likes / peak day, then the top 3 posts by likes for every
day — that list is the event timeline for the doc.

### 3. Label every post (subagents)

```
node $S/chunk.mjs <clean.json> --size 300 --out <scratch>
```

Every clean post lands in one `chunkN.json`. Spawn `general-purpose` Sonnet
labelers, one chunk each, 8 per message; start the next wave when one finishes. The
prompt names the app's official handles (current and former), products and
competitors, and asks for two files per chunk:

- `labelsN.json`: for every post `{id, about, sentiment, topic, point}` —
  `about` true only if the post is about the app itself; `sentiment` like /
  dislike / neutral / noise; `topic` from a fixed list the prompt gives (plus free
  additions); `point` ≤ 12 words.
- `summaryN.md`: top 5 topics, top 5 likes, top 5 dislikes, each with post ids and
  a verbatim quote ≤ 25 words. If the labeler's harness blocks writing `.md`, it
  returns the summary in its reply; save it as `summaryN.md` yourself.

### 4. Aggregate and verify (script)

```
node $S/aggregate.mjs <clean.json> <scratch> [--top 300]
```

Prints how many chunks came back; sentiment over all posts and over the top-liked
(volume share vs attention share); like and dislike counts per topic with unique
authors; the interested-party share of likes (reward earners, token promoters,
official and partner accounts); and, for every summary, unknown ids and quotes that
are not a substring of any post. Rank like/dislike points by their label counts;
use the summaries only to pick quotes. Take the final quote text by id from
`clean.json`, never from a labeler's paraphrase, and link it as
`https://x.com/<author>/status/<id>`.

### 5. Charts (script)

```
echo '{"out_dir":"<slug>/images","charts":[
  {"type":"bar","file":"<slug>-hot-topics.png","title":"热点话题（提及条数）","labels":[...],"values":[...],"color":"#2a78d6"},
  {"type":"daily","file":"<slug>-daily-volume.png","title":"每日提及量与当天事件","days":["09-02",...],"values":[...],"events":{"09-10":"App Store 下架"}},
  {"type":"bar","file":"<slug>-likes.png","title":"最喜欢什么","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<slug>-dislikes.png","title":"最讨厌什么","labels":[...],"values":[...],"color":"#eb6834"}
]}' | $S/render_charts.py /dev/stdin
```

Chinese labels, transparent background, dual-mode gray ink, title only. Open each
PNG and check: no label collisions, headroom above the tallest bar.

### 6. Write `reception.md` (concise Chinese, result only)

```
# <App> 推特口碑（<start> → <end>）

<one line: days, raw count, accounts; clean count after dropping bots/mass-tags/dupes>

- 噪音占比 one line
- 差评 vs 好评 one line（全量 vs 高赞）
- 好评里利益相关方占比 one line

## 一、热点在哪儿
![](images/<slug>-hot-topics.png)
one sentence: the two or three threads everything falls into
![](images/<slug>-daily-volume.png)
- MM-DD event, one line
  > @handle：[verbatim text](url)

## 二、最喜欢什么
![](images/<slug>-likes.png)
1. point, one line (say so when the speakers are interested parties)
   > @handle：[verbatim text](url)

## 三、最讨厌什么
![](images/<slug>-dislikes.png)
1. …

one closing sentence: the common thread of the dislikes
```

Rules:
- No 数据源 / 方法 / 可信度 sections, no process narration, no agent counts.
- Quote line is exactly `> @handle：[text](url)`: link on the text, handle plain,
  no like counts or any number next to the handle, no italics. Every quote line
  carries text.
- Tight prose; parentheses are rare. Charts sit at the top of their section.

### 7. Ship

`git add` `reception.md` and `images/` only, commit, push.

## Requirements

- Node ≥ 20 for the `.mjs` scripts; `uv` for `render_charts.py` (matplotlib);
  CJK font at `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Tests

```
node --test skills/analyze-tweets/scripts/test_analyze_tweets.mjs
```
