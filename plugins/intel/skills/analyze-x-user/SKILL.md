---
name: analyze-x-user
description: Profile ONE X/Twitter account from its own timeline (tweets.jsonl + replies.jsonl left by fetch-x-user-posts) into x/kols/<user>/profile.md under the intel output folder — who the account is, what it talks about most, the tokens and people it pushes, its posting behaviour and interests, read from its own posts. Labels each post with the analyze-x-mentions labeler, then writes a single-account profile. Use when asked to profile / 画像 one KOL from their fetched timeline. NOT for the accounts mentioning an app (use analyze-x-users) and NOT for fetching the posts (use fetch-x-user-posts).
---

# Analyze X user

This skill turns one account's own timeline into `profile.md`: who the account is, what it talks
about most, which tokens and people it pushes, how and when it posts, and what motivates it, read
from its own posts only, with no aggregation across accounts. It mirrors `analyze-x-users`, which
segments the crowd mentioning an app; here the labels describe the account's own posts (what kind,
about which asset, which way, with what stake). The numbers come from the archive, the per-post
labels from the `analyze-x-mentions` labeler run under this skill's spec, and the qualitative read
from the representative posts you read yourself.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
commands run scripts from the sibling `analyze-x-mentions` and `fetch-x-mentions` skills, so keep
the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

The commands below run from any working directory and use these names, with `<user>` the account
and `<scratch>` a scratch directory:

```
S="$SKILL_DIR/scripts"
T="$SKILL_DIR/../analyze-x-mentions/scripts"
STATE="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" state)"
OUT="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" output)"
U="$STATE/x/kols/<user>"
O="$OUT/x/kols/<user>"
VOCAB="$STATE/x/kols/vocab.json"
```

`$U` is the archive: `tweets.jsonl` and `replies.jsonl` from `fetch-x-user-posts`, and next to them
this skill's `profile.json` (the stats) and `labels.jsonl` (one line per post, kept so a rerun
labels only new posts). `$O` gets `profile.md` and `images/`. `$VOCAB` holds the topics and
interests seen across accounts; assets stay per account. Expand `~` to the absolute home path in
JSON arguments.

## Environment variables

No X credentials are needed to analyze an existing archive.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory the shared labeler uses; default `~/.codex` | No | Shell environment before running the command; no automatic `.env` loading |
| `INTEL_STATE_DIR` | State root holding the archive and vocabulary; default `~/.local/state/intel` | No | `~/.config/intel/.env` |
| `INTEL_OUTPUT_DIR` | Output root; profiles go under `x/kols/`; default `~/Documents` | No | `~/.config/intel/.env` |

## Setup

The skill needs Node 20 or later, the shared `analyze-x-mentions` labeler scripts (`clean.mjs`,
`chunk.mjs`, `run-labels.mjs`, `merge-labels.mjs`) and its `render_charts.py`, which needs `uv` and
matplotlib.

## Deterministic profile

```
node $S/profile-user.mjs $U --out <scratch>
```

This prints and writes `<scratch>/profile.json`: volume (tweets vs replies, reply ratio), cadence
(active days, span, posts per active day, posting hours in UTC), engagement (average, median, max),
the `$cashtags` and `@handles` the account pushes most, top hashtags, linked domains, languages and
the 15 most-engaged posts. It is the objective backbone of the profile; read it before writing
anything.

## Label the posts

The labeler is `analyze-x-mentions`'s, with gpt-6-luna through `codex exec`, but the prompt is
this skill's: `$S/kol-spec.mjs` holds the rules, fields and answer schema, and `$VOCAB` the
vocabulary. Each post gets these fields:

- `about`: has content of its own (a view, a call, a trade, a story) vs gm, emoji or one word
- `kind`: call, analysis, pnl, news, promo, banter or noise
- `topic`: the subject (market-macro, token-call, exchange-news, industry-drama, …)
- `asset`: the ticker or project the post is about, `market` for a market-wide view, or `none`
- `stance`: bullish, bearish or neutral toward the asset, `none` when there is no asset
- `point`: the claim in at most 12 words
- `interest`: the stake the post itself shows (own-token, referral, sponsored, exchange-affiliate,
  paid-group, creator-rewards, airdrop-farming, team-member) or null

When the account has too few substantive posts to profile (say under 30), skip labeling and say so
in the doc. Otherwise combine the two streams and chunk them, skipping posts already in
`labels.jsonl`:

```
cat $U/tweets.jsonl $U/replies.jsonl > <scratch>/all.jsonl
node $T/clean.mjs <scratch>/all.jsonl --out <scratch>/clean.json
node $T/chunk.mjs <scratch>/clean.json --size 500 --out <scratch> --labels $U/labels.jsonl
```

Write `<scratch>/account-facts.md` for the labeler with facts only, since the field definitions come
from the spec: the account's display name, its languages, what it is known for, the chains,
exchanges and tokens it is tied to, the handles it talks to most (from `profile.json`) and the
noise typical of its replies.

Then run the labeler in the background and merge its output into the account's store:

```
node $T/run-labels.mjs --spec $S/kol-spec.mjs --facts <scratch>/account-facts.md --vocab $VOCAB --out <scratch> <scratch>/chunk*.json
node $T/merge-labels.mjs $U/labels.jsonl <scratch> --spec $S/kol-spec.mjs --vocab $VOCAB
```

Chunks of 500 keep the wall time near that of one chunk, about 5 minutes, because the pool runs 20
at once.

## Representative posts

Rerun the profile with the labels so `profile.json` carries the label tallies (kinds, topics,
assets with their bullish and bearish split, interests) and the labels of the 15 top posts:

```
node $S/profile-user.mjs $U --labels $U/labels.jsonl --out <scratch>
```

Then dump the posts each section is written from, and read the file yourself, with no subagent and
no hand sampling:

```
node $S/reps.mjs $U --out <scratch>/reps.txt [--per 15] [--apps <archive-slugs>]
```

It writes one block per section, chosen by label: the top posts of each of the 10 biggest topics,
the market and BTC posts in date order, every post with a stake grouped by stake, calls, promos,
the top assets, the accounts it @-mentions most and, for the comma-separated archive slugs given
with `--apps`, every post naming that trading app (none by default). A 3000-post account gives
about 1200 lines.

## Charts

Render five PNGs into `$O/images/` with the `analyze-x-mentions` chart script, taking the values
from `profile.json` and `labels.jsonl`. Sum the topic groups the doc uses over `labels.jsonl`, not
over the top-15 list in `profile.json`. The assets chart shows the top 10 assets with at least 5
bullish plus bearish posts.

```
echo '{"out_dir":"x/kols/<user>/images","charts":[
  {"type":"bar","file":"<user>-kinds.png","title":"帖子类型","labels":["闲聊","分析",...],"values":[...],"color":"#2a78d6"},
  {"type":"bar","file":"<user>-topics.png","title":"聊什么","labels":["<grouped topic>",...],"values":[...],"color":"#2a78d6"},
  {"type":"bar","file":"<user>-interests.png","title":"利益","labels":["X 创作者分成","返佣链接",...],"values":[...],"color":"#eb6834"},
  {"type":"grouped","file":"<user>-assets.png","title":"标的与立场","labels":["大盘","BTC",...],
   "series":[{"name":"看多","values":[...],"color":"#1baf7a"},{"name":"看空","values":[...],"color":"#eb6834"}]},
  {"type":"bar","file":"<user>-mentions.png","title":"来往最多","labels":["@handle",...],"values":[...],"color":"#2a78d6"}
]}' | $T/render_charts.py /dev/stdin
```

Open every PNG and check its labels.

## Write the profile

Write `$O/profile.md` in Chinese, concrete and from evidence only, in this shape:

```
# @<user> 画像（<first_post> → <last_post>）

## 一句话
**<who this account is and what it is really doing on X>**

## 概况

![帖子类型](images/<user>-kinds.png)

- 帖子类型（<labeled> 条有标签）：<kinds as shares>
- 发帖：<posts> 条（原创 <tweets> / 回复 <replies>，回复占比 <reply_ratio>）；活跃 <active_days> 天 / 跨度 <span_days> 天，<posts_per_active_day> 条/活跃日
- 互动：平均 <avg>，中位 <median>，最高 <max>；活跃时段（UTC）<busiest hours>，即北京时间 <hours>
- 语言：<languages>
- 月度：<peak month and why>，<range>，<partial last month noted>

## 聊什么

![聊什么](images/<user>-topics.png)

1. <topic group>（<label names>），<count> 条。<what exactly>
   > @<user>：[verbatim text](url)
(5 groups ranked by count; the market / BTC group carries the bullish / bearish counts and a
short 立场 paragraph with dated quotes: the thesis, whether it changed, where it stands at the end)

## 利益与立场

![利益](images/<user>-interests.png)
![标的与立场](images/<user>-assets.png)

- <stake>，证据：<count> 条带 <signal>，<platforms / codes / amounts the posts show>
  > @<user>：[verbatim text](url)
(one bullet per stake the posts show: creator-rewards, referral, sponsored, paid-group, own
project / token; say plainly when a stake is absent)
- 主推的币 / 项目（asset 计数，多/空）：<top assets>。<which are positions, which are jokes or hype>
- 喊单：<count> 条，<what they call>

## 行为模式

![来往最多](images/<user>-mentions.png)

- 谁来往最多：<top mentions with counts>。<which circle, how it treats each>
  > @<user>：[verbatim text](url)
- 节奏与语气：<when, how much, what register, what for>

## 代表作
- <3–5 highest-signal posts, one line each + quote link>

## 用处
- <what the posts show about trading apps it uses or rates, with quotes>
- <the levers that move it, ranked by evidence>
- <risks for us>

## TL;DR
- 最多 5 条，大白话

可信度：<one line: how many posts labeled, numbers from labels + archive, quotes id-checked>
```

Every claim carries its number or its quote. A quote line is exactly `> @<user>：[text](url)`, with
the link on the verbatim text and Chinese posts quoted in Chinese. Say "有返佣" only with a referral
post, and "广告 / sponsored" only when a post says so. Take a follower count from
`$STATE/x/crm-followers.json`. Narrate no method, model or process outside the 可信度 line.

Before shipping, run the quote check; it must report 0 bad, meaning every quoted id exists, its
handle matches and the quote is a verbatim substring. Fix the doc, never the check.

```
node $S/check-quotes.mjs $O/profile.md $U
```

## Ship

Return the paths to `$O/profile.md`, `$O/images`, `$U/profile.json`, `$U/labels.jsonl` and
`$VOCAB`. `clean.json`, the chunks and the `labelsN.json` files are scratch.

## Profiling a roster

This skill profiles one account. For a roster, run it once per account, each in its own subagent
from a bounded pool, with explicit file paths and a distinct output directory per account. This is
the expensive downstream step, not part of fetching, so never fan labeling out to a whole roster
unless asked.

## Tests

```
node --test "$SKILL_DIR/tests/test_profile_user.mjs" "$SKILL_DIR/tests/test_reps.mjs"
```
