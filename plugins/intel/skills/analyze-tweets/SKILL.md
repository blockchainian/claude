---
name: analyze-tweets
description: Turn a fetched X/Twitter mentions archive (tweets.jsonl from fetch-x-mentions) into a concise, data-driven Chinese reception doc plus charts — hot topics with a dated timeline, what people like, what people dislike, each ranked by frequency over every post and backed by id-verified quotes. Use when asked to analyze/分析 what X is saying about an app, brand or protocol from an existing mentions dataset. NOT for fetching the tweets (the archive must already exist) and NOT for App Store reviews (use analyze-appstore-reviews).
---

# Analyze Tweets

One app's X mentions → a short, evidence-only reception doc: 热点、最喜欢、最讨厌.
Every number comes from the JSON; every post is labeled by local models (no sampling);
every quote is verbatim and id-verified. The doc reads in one pass: result only, no method or process
narration.

## Input & output

- **Input**: `mentions/<slug>/tweets.jsonl` as written by `fetch-x-mentions.mjs`
  (one tweet per line with `id, author, text, created_at, likes, replies, lang, url`;
  `clean.mjs` dedups by id). Run on the whole archive; pass `--since/--until` to
  `topics.py` to analyze a window of it. The doc title states the window.
- **Output**, next to the input: `mentions/<slug>/reception.md` + `images/`. A windowed
  run writes `reception-<since>.md` so it never overwrites the all-time doc.

`S="${CLAUDE_PLUGIN_ROOT}/skills/analyze-tweets/scripts"` below.

## Procedure

### 1. Clean (script)

```
node $S/clean.mjs <tweets.jsonl> --out <scratch>/clean.json
```

Always the whole archive: the topic cache below is all-time, and a window is applied
in step 2. Drops bot alert templates (`Route:`, `MIGRATION`, `CTO SIGNAL`, `WALLET
FLOW CHECK`, `Quick Buy`, `dm us`; extend with `--bot-pattern`), posts tagging ≥ 6
handles, duplicates after stripping handles/urls, and texts under 8 chars. Prints
raw/clean counts, date range, account count, top authors, clean count by day, and
what was dropped. The raw→clean numbers become the doc's scope line.

### 2. Topics and sentiment on every post (script)

```
$S/topics.py <scratch>/clean.json --cache ~/.cache/analyze-tweets/<slug> \
  --names mentions/<slug>/topics.json --out <scratch> \
  [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--refit]
```

Local models, no keyword list: `bge-small-en-v1.5` embeds every English post,
BERTopic (UMAP + HDBSCAN, fixed seed) clusters them, `twitter-roberta-base-sentiment`
scores each one like / dislike / neutral. Everything is cached by post id under
`--cache`, so a rerun only embeds and assigns the posts added since last time
(under half a minute); the first run, or `--refit`, re-clusters on a 100k sample
and assigns every post (measured 3.8 min on 173k posts; roughly 15–20 min per
million, estimated). HDBSCAN leaves over half
of all tweets unclustered; those go to their nearest cluster, and the share that
were outliers is printed as the refit signal. Non-English posts are the row
`其他语言`. `--since/--until` only filter the printed table, timeline and
`labels0.json`; the cache and the model stay all-time, so a weekly window and the
full history use the same topic names.

Prints per topic hits / unique authors / likes / peak day / keywords, then the top 3
posts by likes for every day — that list is the event timeline for the doc. Writes
`<scratch>/labels0.json` (`{id, about, sentiment, topic}`; `about` = English and
clustered) for step 3 and `<scratch>/clusters.json` with the keywords and 10
most-liked posts of every cluster that has no name yet.

**Name the clusters yourself** (no API calls from scripts): read `clusters.json`
and add every cluster to `--names` as `"<id>": {"name": "<中文标签>", "keywords":
[...]}` (copy the keywords from `clusters.json`) — short, what the posts are about;
merge near-duplicates by giving them the same name; spam and banter get `噪音` —
then rerun the command. The names file is the one state worth committing: after a
`--refit` the cluster ids change, and a new cluster inherits the name of the old one
whose keywords it shares (Jaccard ≥ 0.5); only the rest need naming again, and
names whose clusters vanished are dropped from the file.

The run prints the outlier share of the new posts. When it is clearly above the
all-cached share, a new topic has appeared: rerun with `--refit`, then name the new
clusters.

### 3. Aggregate and verify (script)

```
node $S/aggregate.mjs <scratch>/clean.json <scratch> [--top 300]
```

Prints sentiment over all posts and over the top-liked (volume share vs attention
share); like and dislike counts per topic with unique authors; and the
interested-party share of likes (reward earners, token promoters, official and
partner accounts). Rank like/dislike points by their label counts. Take every quote
text by id from `clean.json` and link it as `https://x.com/<author>/status/<id>`;
pick from the top-liked posts of the topic (the `clusters.json` examples are a good
start) and drop any whose sentiment label reads wrong.

### 4. Charts (script)

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

### 5. Write `reception.md` (concise Chinese, result only)

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
- No 数据源 / 方法 / 可信度 sections, no process narration, no model or agent talk.
- Quote line is exactly `> @handle：[text](url)`: link on the text, handle plain,
  no like counts or any number next to the handle, no italics. Every quote line
  carries text.
- Tight prose; parentheses are rare. Charts sit at the top of their section.

### 6. Ship

`git add` `reception.md`, `images/` and `topics.json` only, commit, push.
`clean.json`, `labels0.json` and `clusters.json` are scratch; the cache under
`~/.cache/analyze-tweets/` (embeddings, sentiment, the 500 MB model) is rebuilt by
`--refit` and never committed.

## Requirements

- Node ≥ 20 for the `.mjs` scripts; `uv` for the Python scripts (they declare their
  own dependencies; the first `topics.py` run installs torch and friends, about 1 GB,
  and downloads the two models, about 600 MB, from Hugging Face without a token);
  Apple Silicon or CUDA for speed; CJK font at
  `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Tests

```
node --test skills/analyze-tweets/scripts/test_analyze_tweets.mjs
skills/analyze-tweets/scripts/test_topics.py
```
