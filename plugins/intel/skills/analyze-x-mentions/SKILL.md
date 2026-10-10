---
name: analyze-x-mentions
description: Turn a fetched X/Twitter mentions archive (tweets.jsonl from fetch-x-mentions) into a concise, data-driven Chinese reception doc plus charts — hot topics with a dated timeline, what people like, what people dislike, each ranked by frequency over every post and backed by id-verified quotes. Use when asked to analyze/分析 what X is saying about an app, brand or protocol from an existing mentions dataset. NOT for fetching the tweets (the archive must already exist) and NOT for App Store reviews (use analyze-appstore-reviews).
---

# Analyze X mentions

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
S="$SKILL_DIR/scripts"
X="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" state)/x"
OUT="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" output)"
```

`$X` is the archive directory under the state root and `$OUT` the output root. Commands may run from
any working directory. Expand `~` to the absolute home path in JSON arguments.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default `~/.codex` | No | Shell environment before running the command; no automatic `.env` loading |
| `INTEL_STATE_DIR` | State root; the archive, `labels.jsonl` and `vocab.json` live under `x/`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |
| `INTEL_OUTPUT_DIR` | Output root; docs and charts go under `x/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

The labeler reads `auth.json` from the Codex login directory and copies it into a temporary private
Codex home. Analyzing an existing archive needs no X credentials.

## Setup

The `.mjs` scripts need Node 20 or later, and `render_charts.py` needs `uv` (it installs matplotlib
itself) and the CJK font at `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## What the skill produces

The skill turns one app's X mentions into an evidence-only reception doc: an overview, what users
like and dislike about the app, what they ask for, the timeline, and what it means for us. Every
number comes from the JSON, every post is read and labeled by a model, and every quote is verbatim
and id-verified. Never sample, filter by keyword or cluster with a model, because those drop the
concrete content. Concrete beats short: name the feature, the bug, the number, the date.

The input is `$X/<slug>/tweets.jsonl` as written by `fetch-x-mentions`, one tweet per line with `id,
author, text, created_at, likes, replies, lang, url`. Run on a finished archive, and pass
`--since/--until` to analyze a window of it; the doc title states the window.

The skill writes `x/<slug>/reception.md` and `x/<slug>/images/` under the output root. A windowed
run writes `reception-<from>..<to>.md` instead, with the shared date prefix written once
(`reception-2026-09-01..22.md`, `reception-2026-08-15..09-22.md`,
`reception-2024-01-01..2028-01-20.md`), so it never overwrites the all-time doc.

Two stores sit under the state root and are committed. `$X/<slug>/labels.jsonl` holds one line per
post (`id, about, sentiment, topic, feature, point, request, interest`), so the next run labels only
posts it has not seen and any window can be reported from the store without labelers. `$X/vocab.json`
(`{topics, features, interests}`) is one vocabulary shared by every app: it seeds each run's prompt
and grows after every run. The apps overlap heavily, so keep one shared list rather than one per app,
which goes stale.

## Runs

Every run is incremental through the store:

- **First run on an app**: there is no `labels.jsonl` yet, so every clean post is chunked and
  labeled, and the all-time `reception.md` is written.
- **New data arrived** ("analyze this week's <app> tweets"): run Clean, Timeline, Label and
  Aggregate on the whole archive. `chunk.mjs` skips every post already in `labels.jsonl`, so only the
  new posts go to labelers. Then aggregate with `--since/--until` and write
  `reception-<from>..<to>.md`, or aggregate without a window to refresh `reception.md`.
- **Report only** ("report on August from what we have", or a rewrite after a template change): run
  Clean, Timeline, Aggregate, Charts and Write; no labelers, and every number comes from the store.

## Clean

```
node $S/clean.mjs <tweets.jsonl> --out <scratch>/clean.json [--since YYYY-MM-DD] [--until YYYY-MM-DD]
```

The script dedups by id and drops bot alert templates (`Route:`, `MIGRATION`, `CTO SIGNAL`,
`WALLET FLOW CHECK`, `Quick Buy`, `dm us` and similar; extend with `--bot-pattern`), posts tagging 6
or more handles, duplicates after stripping handles and URLs, and texts under 8 characters. It prints
raw and clean counts, the date range, the account count, top authors, the clean count by day and what
was dropped. The raw→clean numbers go under the volume chart in the timeline section.

## Timeline

```
node $S/aggregate.mjs <clean.json> <scratch> --timeline [--since D] [--until D]
```

This prints the top 3 posts by likes for every day of the window, the raw material of the timeline
section. There is no keyword list: what users talk about comes from the labels.

## Label

```
node $S/chunk.mjs <clean.json> --size 1500 --out <scratch> --labels $X/<slug>/labels.jsonl [--since D] [--until D]
```

Only posts missing from `labels.jsonl`, and inside the window if one is given, are chunked into
`chunkN.json` and `chunkN.tsv` (one post per line: id, author, likes, date, lang, text).

Write `<scratch>/app-facts.md` once: the app's official handles (current and former), founder and
team, products and features, competitors, and the noise common in its mentions (referral spam,
giveaway begging, bot alerts, user slang such as chain nicknames). Put nothing else there; the field
definitions and the vocabulary come from the script.

### Codex labeler

The default labeler is gpt-6-luna through `codex exec`, driven by `run-labels.mjs` over a parallel
pool of 20 (`--par`). The rules, fields and answer schema come from `$S/mentions-spec.mjs`, the
default `--spec`; other skills pass their own spec to the same scripts.

```
node $S/run-labels.mjs --facts <scratch>/app-facts.md --vocab $X/vocab.json --out <scratch> <scratch>/chunk*.json
```

Run it in the background: about 100 chunks take about an hour. Each call runs in a private
`CODEX_HOME` with no user config, AGENTS.md, plugins, hooks or tools, and answers by post number
under a JSON schema, because small models mistype 19-digit ids. Calls use the Fast service tier
(about 1.5x the speed at 2x the price, still about a tenth of gpt-6-sol's cost); a model that does not
advertise the tier silently downgrades. `run-labels.mjs` always passes the default tier; to opt out
on a single chunk, run `label-codex.mjs <chunkN.json> --facts … --vocab … --out … --service-tier
standard`. Keep reasoning effort low: higher effort made replies drop out and likes inflate.

Each chunk writes `labelsN.json` and prints
`chunk N: <posts> labeled, <noise%> noise, <about%> about; <tool calls>, <seconds>, <usage>`. Each
pass also logs `chunk N pass P/M: +<got>/<asked>, <left> left` to stderr, so a chunk that early-stops
(returns a valid but short answer, about 1 in 10 on 1500 posts) shows at once as a low `+got/asked`
rather than a silent stall.

Recovery is divide and conquer, not more same-size retries: early-stop is random and does not depend
on size, so re-asking the same big chunk only lengthens the tail. When a chunk fails, the script
splits it about 3 ways and re-queues the parts in parallel, down to a depth cap (1500 → ~500 → ~167);
parts at the cap get full passes. Anything still unlabeled at the cap is printed with its ids and the
script exits non-zero. Measured on the Fast tier at low effort: 167 posts take about 87 s, 500 about
4 min, 1500 about 12 min.

Two Luna habits to correct when reading the results. Luna labels most content-free about=false posts
`irrelevant` rather than `noise`, so the doc reports one about=false share and never the split. Luna
over-uses `mobile-app` as a feature for "I use the app"; fold it with `--rename` when it dominates.

### Sonnet labeler

When the ChatGPT plan quota is out, label with Sonnet subagents in Claude Code. In Codex, report the
quota block and keep the chunks for a later resume; never switch models silently or assume Sonnet is
available there.

Chunk with `--size 2000` and spawn `general-purpose` Sonnet labelers, one chunk each and about 10 in
flight, starting the next when one finishes. Do not go above about 3000 posts per chunk, because
Sonnet's window is 1M and its output cap 128k; below 2000 the fixed 59k-token setup per agent
dominates. A 2000-post chunk is about 110k tokens of posts in and 70k of labels out, but a labeler
runs 40–70 tool turns and re-sends its context each turn, about 15M input tokens (roughly $5 at
Sonnet prices, mostly cache reads) per chunk.

Write the prompt once to `<scratch>/PROMPT.md`, holding the app facts and the field definitions
below, and point each labeler at it. The prompt says:

- First print all the posts, in batches of 100 as `id \t author \t likes \t date \t lang \t text`,
  into batch files under a private `work_N/` directory (never a shared one: labelers run side by
  side), then read every batch file; host tools truncate large results, so reading is batched
  whatever the prompt says. After each batch, write that batch's labels as DATA, a file with one
  object per post id holding the label decided while reading, never a classifier: no keyword rules,
  no regex, no function that derives labels from text. A final script only concatenates the batch
  data files, checks ids and order, and writes `labelsN.json`. A prompt that asked for "one script
  holding all labels" made every labeler compress its judgments into regex rules; the per-batch data
  files keep the labels per post.
- Then write two files:
  - `labelsN.json`: one object per post, in chunk order, every id present:
    `{id, about, sentiment, topic, feature, point, request, interest}`.
    - `about`: true only if the post is about the app itself; a reply that only carries the app's
      handle from the thread and talks about something else is false.
    - `sentiment`: for about=true, like / dislike / neutral / noise (noise: about the app but
      content-free, such as "gm @app"); for about=false, `irrelevant` (talks about something else:
      another token, a person in the thread) or `noise` (content-free: one-word replies, emoji,
      giveaway begging, bot alerts). The doc reports one about=false share for both.
    - `topic`: what the post is about as a subject people discuss (the event, the company, the
      ecosystem, the culture). The prompt lists the topics in `vocab.json` as examples (first run:
      product-features, outages-execution, fees-pricing, insiders-manipulation, scams-rugs,
      verification-listing, competition, funding-revenue-growth, chain-integrations,
      kols-celebrities, challenges-giveaways, token-communities, security-custody,
      regulation-regions, culture-memes, support-team) and says: if none fits, write your own.
      Never an `other` bucket.
    - `feature`: which part of the app the post is about; the prompt lists the features in
      `vocab.json` as examples (first run: fees, cross-chain-balance, copy-trading, leaderboard,
      thesis, clans, callouts, verification, limit-orders, mobile-app, web-app, ui, stability,
      custody-wallet, support, referral, apple-pay-onboarding, livestream, followers-social,
      chain-integration), and if none fits, write your own; `none` when no part of the app applies.
    - A topic or feature the labeler writes itself must be a 2–4 word English noun phrase, lowercase
      with hyphens, specific enough to tell apart from the examples, and reused for every post about
      the same thing (check the examples first, then your own earlier names).
    - `point`: at most 12 words saying what the post claims (the bug, the number, the complaint),
      never the feature name alone.
    - `request`: at most 12 words when the post asks to add, fix, change or remove something, else
      null.
    - `interest`: null when the speaker has no stake; otherwise the kind of stake, from the interests
      in `vocab.json` (first run: referral — posts a code or link; creator-rewards — earns callout /
      thesis rewards; token-team — promotes their own token; official-partner — the company, staff,
      partners; paid-promotion); write your own if none fits.

    Non-English posts are labeled like the rest, with text fields in English. Topic × sentiment gives
    the hot-topics section, feature × sentiment gives likes, dislikes and requests, and `interest`
    gives the interested-party share.
  - `summaryN.txt` (plain text, not `.md`, because the harness refuses subagent report-style
    markdown): sections LIKES, DISLIKES and REQUESTS with the top 5 points by post count, each with
    ids and one verbatim quote of at most 25 words (Chinese posts quoted in Chinese, never spanning a
    t.co link), then FACTS: numbers, launches, outages, funding, partnerships and store removals,
    each with its id.
- Reply with one line: `chunk N: <posts> labeled, <noise%> noise, <about%> about`.

Before merging, audit each `work_N/`. A `.py` there with `re.compile`, `re.search` or `in text`-style
rules, or batch files whose ids add up to fewer than the chunk's posts, means the chunk was
classified by rules: discard it and rerun that chunk.

### Merge labels

Both labelers write the same `labelsN.json` shape from the same field definitions. Fold the chunks
into the store and grow the vocabulary:

```
node $S/merge-labels.mjs $X/<slug>/labels.jsonl <scratch> --vocab $X/vocab.json [--rename topic:old=new ...]
```

The script merges by id (a relabeled id overwrites), applies the aliases already in `vocab.json`,
adds every new topic, feature or interest value that reaches 1% of this batch's about=true posts to
`vocab.json`, and prints the values below that line with their counts. Read the printed lists and
fold any new value that means the same as an existing one with `--rename topic:old=new`; the rename is
field-scoped, rewrites the store and is kept as an alias so later runs fold it automatically. The
other values stay in the labels but not in the vocabulary. Commit `vocab.json` with the report, since
the next run on any app seeds its prompt from it.

## Aggregate and verify

```
node $S/aggregate.mjs <clean.json> <scratch> --labels $X/<slug>/labels.jsonl [--since D] [--until D] [--top 300]
```

The script prints:

- how many window posts have a label;
- topics with like / dislike / neutral counts and the peak month of each;
- sentiment over all posts and over the top-liked (volume share against attention share);
- like, dislike and request counts per feature, with unique authors;
- the most frequent `point` and `request` texts;
- the interested-party share of likes by kind of `interest`;
- for every `summaryN.txt` present (Sonnet labeler only), unknown ids and quotes that are not a
  substring of any post.

Re-run any chunk whose noise share is far below the others: a labeler that marks one-line reply
banter as neutral instead of noise inflates the "about" counts. Rank likes, dislikes and requests by
their label counts, and pick quotes from the top `point` texts and the summaries when present. Take
the final quote text by id from `clean.json`, never from a labeler's paraphrase, and link it as
`https://x.com/<author>/status/<id>`.

## Charts

```
echo '{"out_dir":"x/<slug>/images","charts":[
  {"type":"bar","file":"<slug>-hot-topics.png","title":"热点话题（提及条数）","labels":[...],"values":[...],"color":"#2a78d6"},
  {"type":"daily","file":"<slug>-daily-volume.png","title":"每日提及量与当天事件","days":["09-02",...],"values":[...],"events":{"09-10":"App Store 下架"}},
  (a window longer than ~3 months uses months as days: `<slug>-monthly-volume.png`, "每月提及量与当月事件", "days":["2025-01",...])
  {"type":"bar","file":"<slug>-likes.png","title":"用户喜欢 App 的什么","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<slug>-dislikes.png","title":"用户不满 App 的什么","labels":[...],"values":[...],"color":"#eb6834"},
  {"type":"bar","file":"<slug>-requests.png","title":"用户想要什么","labels":[...],"values":[...],"color":"#8a5cd6"}
]}' | $S/render_charts.py /dev/stdin
```

A relative `out_dir` resolves against the output root. The charts have Chinese labels, a transparent
background, gray ink that reads in light and dark mode, and a title only. Event labels on the daily
chart sit horizontally above their day, staggered on three levels with a leader line, so keep each
event name to about 8 characters and mark at most ten days. A `$` in a label is matplotlib mathtext;
escape it. Open each PNG and check that no labels collide and the tallest bar has headroom.

## Write the doc

Write `$OUT/x/<slug>/reception.md` (or the windowed name) in Chinese, concrete and evidence only:

```
# <App> 推特口碑（<start> → <end>）

## 一、概况
<three short paragraphs of prose, no bullets, no counts, no percentages, no window,
no method: an overview a reader skims before the findings>
<p1: what the app is, as the posts describe it — product, chains, how people use it>
<p2: in plain words, what people like about it, what they complain about, and the
one or two topics that dominate the conversation; the numbers live in 二–五>
<p3: the company's history as the posts tell it: funding, launches, user milestones>

## 二、热点在哪儿
![](images/<slug>-hot-topics.png)
1. <topic>: <count> 条, 好评 <n> / 差评 <n>, 峰值 <month>, <what the peak was about>
   > [@handle](https://x.com/<handle>)：[verbatim text](url)

## 三、用户喜欢 App 的什么
![](images/<slug>-likes.png)
1. <feature or behavior>: <what exactly they praise>, <count> 条 / <authors> 人（say when the speakers are interested parties）
   > [@handle](https://x.com/<handle>)：[verbatim text](url)

## 四、用户不满 App 的什么
![](images/<slug>-dislikes.png)
1. <feature or behavior>: <the specific bug, fee, delay, rule>, with the numbers users cite
   > [@handle](https://x.com/<handle>)：[verbatim text](url)

## 五、用户想要什么
![](images/<slug>-requests.png)
1. <request>: add / fix / change what, <count> 条
   > [@handle](https://x.com/<handle>)：[verbatim text](url)

## 六、时间线：声量、情绪、关键事件
![](images/<slug>-daily-volume.png)
- the scope in one line: the date range, the total posts, and the about=false (off-topic / noise) share — no cleaning or pipeline details
- MM-DD event, one line, with the number it moved
  > [@handle](https://x.com/<handle>)：[verbatim text](url)
- how like/dislike share moved over the window (by month or quarter)
- facts worth keeping: funding, users, revenue, integrations, outages, store removals (id-linked)

## 七、对我们的启示
- one line per lesson, each tied to a finding above: what to copy, what to avoid, what users will ask us for

## TL;DR
- at most 5 bullets, plain words
```

The doc follows these rules:

- It is a report on the app, not a record of what you did to the data: no 方法 or 可信度 sections, no
  process narration, no agent or model talk, and no notes about removing bots, cleaning or filtering.
- A quote line is exactly `> [@handle](https://x.com/<handle>)：[text](url)`: the handle links to the
  account and the text to the post, with no like counts or any number next to the handle and no
  italics. Every quote line carries text, and Chinese posts are quoted in Chinese.
- Any @handle in the prose is a link too: `[@handle](https://x.com/<handle>)`.
- Every claim in 二–七 names the feature, the bug, the number or the date. "故障 bug" or "骂战" is not a
  finding; "买入后 12 小时无法卖出，09-08 当天 41 条" is. 一 is the exception: prose without numbers.
- Charts sit at the top of their section.

Return the paths to `reception.md` and `images/` under the output root and to `labels.jsonl` under the
state root. `clean.json`, the chunks, `labelsN.json` and `summaryN.txt` are scratch.

## Tests

```
node --test "$SKILL_DIR/tests/test_analyze_tweets.mjs"
```
