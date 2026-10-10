---
name: analyze-x-user
description: Profile ONE X/Twitter account from its own timeline (tweets.jsonl + replies.jsonl left by fetch-x-user-posts) into x/kols/<user>/profile.md under the intel output folder — who the account is, what it talks about most, the tokens and people it pushes, its posting behaviour and interests, read from its own posts. Labels each post with the analyze-x-mentions labeler, then writes a single-account profile. Use when asked to profile / 画像 one KOL from their fetched timeline. NOT for the accounts mentioning an app (use analyze-x-users) and NOT for fetching the posts (use fetch-x-user-posts).
---

# Analyze X user (single account)

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The commands run scripts from the sibling
`analyze-x-mentions` and `fetch-x-mentions` skills, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Archive paths below show the default `~/.local/state/intel` root; replace it with the configured `INTEL_STATE_DIR` when set. Docs and charts go under the output root, `INTEL_OUTPUT_DIR` (default `~/Documents`). Expand `~` to the absolute home path in JSON arguments.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment before running the command; no automatic `.env` loading |
| `INTEL_OUTPUT_DIR` | Output root; profiles go under `x/kols/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

The shared `analyze-x-mentions` labeler uses this setting for its existing Codex login. No X credentials are needed to analyze an existing timeline archive.

One account's own timeline → `profile.md`: who this account is, what they talk about most, which
tokens and people they push, how and when they post, and what motivates them — read from their
own posts, no aggregation across accounts. Deterministic stats come from the archive; the kind,
topic, asset, stance and interest of each post come from `analyze-x-mentions`'s labeler run under
this skill's own spec; the qualitative read comes from the representative posts `reps.mjs` picks
by label, read by whoever runs the skill.

This is per-account profiling. It is the mirror of `analyze-x-users` (which segments the *crowd*
mentioning an app); here the subject is the account itself, so the labels describe the account's
own posts (what kind, about which asset, which way, with what stake), not an app.

## Input & output

- **Input**: `~/.local/state/intel/x/kols/<user>/tweets.jsonl` and `.../replies.jsonl` from `fetch-x-user-posts`.
- **Output**: `profile.md` + `images/` in `x/kols/<user>/` under the output root; next to the input, `profile.json`
  (deterministic stats) and `labels.jsonl` (one line per post, committed so a rerun only labels new posts).

Commands may run from any working directory; `S="$SKILL_DIR/scripts"`,
`T="$SKILL_DIR/../analyze-x-mentions/scripts"` (the shared labeler) below. `U=~/.local/state/intel/x/kols/<user>` (the archive), `O="$OUT/x/kols/<user>"` (the doc), with the output root from

```
OUT="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" output)"
```

## Procedure

### 1. Deterministic profile (script)

```
node $S/profile-user.mjs $U --out <scratch>
```

Prints and writes `<scratch>/profile.json`: volume (tweets vs replies, reply ratio), cadence
(active days, span, posts/active day, posting hours in UTC), engagement (avg / median / max),
the `$cashtags` and `@handles` the account pushes most, top hashtags, domains linked, languages,
and the 15 most-engaged posts. Rerun with `--labels $U/labels.jsonl` after step 3 to fold in the
label tallies. This is the objective backbone; read it before writing anything.

### 2. Label the account's own posts (the shared labeler, the KOL spec)

The labeler machinery (`clean.mjs`, `chunk.mjs`, `run-labels.mjs`, `merge-labels.mjs`, gpt-6-luna
through `codex exec`) is `analyze-x-mentions`'s; the prompt is not. `$S/kol-spec.mjs` holds this
skill's rules, fields and answer schema, and `~/.local/state/intel/x/kols/vocab.json` its own vocabulary
(topics and interests seen across accounts; assets stay per account). Fields per post:

- `about` — has content of its own (a view, a call, a trade, a story) vs gm / emoji / one word
- `kind` — call · analysis · pnl · news · promo · banter · noise
- `topic` — the subject (market-macro, token-call, exchange-news, industry-drama, …)
- `asset` — the ticker or project the post is about, `market` for a market-wide view, `none`
- `stance` — bullish / bearish / neutral toward the asset, `none` when no asset
- `point` — the claim in ≤ 12 words
- `interest` — the stake the post itself shows: own-token, referral, sponsored, exchange-affiliate,
  paid-group, creator-rewards, airdrop-farming, team-member, or null

Combine the two streams and chunk them, skipping posts already in `labels.jsonl`:

```
cat $U/tweets.jsonl $U/replies.jsonl > <scratch>/all.jsonl
node $T/clean.mjs <scratch>/all.jsonl --out <scratch>/clean.json
node $T/chunk.mjs <scratch>/clean.json --size 500 --out <scratch> --labels $U/labels.jsonl
```

Write `<scratch>/account-facts.md` for the labeler: the account's display name, language(s), what
it is known for, the chains / exchanges / tokens it is tied to, the handles it talks to most (from
`profile.json`), and the noise typical of its replies. Facts only; the field definitions come from
the spec.

Run the labeler and merge into this account's store:

```
node $T/run-labels.mjs --spec $S/kol-spec.mjs --facts <scratch>/account-facts.md --vocab ~/.local/state/intel/x/kols/vocab.json --out <scratch> <scratch>/chunk*.json
node $T/merge-labels.mjs $U/labels.jsonl <scratch> --spec $S/kol-spec.mjs --vocab ~/.local/state/intel/x/kols/vocab.json
```

Chunks of 500 keep the wall time near one chunk (about 5 min) since the pool runs 20 in flight; skip labeling entirely for an account
with too few substantive posts to profile (say < 30) and note that in the doc. Run `run-labels.mjs` in the background.

### 3. Representative posts (script, read inline)

Rerun `profile-user.mjs $U --labels $U/labels.jsonl --out <scratch>` so `profile.json` carries
the label tallies (kinds, topics, assets with their bullish / bearish split, interests) and the 15
top posts with their labels. Then dump the posts each section is written from and read the file:

```
node $S/reps.mjs $U --out <scratch>/reps.txt [--per 15] [--apps <archive-slugs>]
```

One block per section, chosen by label: the top posts of each of the 10 biggest topics, the
market / BTC posts in date order, every post with a stake grouped by stake, calls, promos, the top
assets, the accounts it @-mentions most, and every post naming a trading app (`--apps`, default
list in the script). About 1200 lines for a 3000-post account; no subagent, no sampling by hand.

### 4. Charts (analyze-x-mentions script)

Five PNGs into `$O/images/`, values from `profile.json` and `labels.jsonl` (the topic groups are
the ones the doc uses, summed over `labels.jsonl`, not over the top-15 list in `profile.json`):

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

Assets: those with at least 5 bullish + bearish posts, top 10. Open every PNG and check the labels.

### 5. Write `profile.md` (Chinese, concrete, evidence only)

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

Rules:
- Quote line is exactly `> @<user>：[text](url)`, link on the text, verbatim, Chinese posts in
  Chinese. Every claim carries its number or its quote. "有返佣" needs a referral post; "广告 /
  sponsored" only when a post says so. A follower count comes from `~/.local/state/intel/x/crm-followers.json`.
- No method / model / process narration except the 可信度 line.
- Before shipping: `node $S/check-quotes.mjs $O/profile.md $U` must report 0 bad (id exists,
  handle matches, quote is a verbatim substring). Fix the doc, never the check.

### 6. Ship

Return the paths to `$O/profile.md`, `$O/images`, `$U/profile.json`, `$U/labels.jsonl` and the vocabulary file. `clean.json`, the chunks and `labelsN.json` are scratch.

## Batch note

This skill profiles **one** account. To profile a roster, run it per account (a subagent each,
a bounded pool using Claude Code's Agent tool or Codex's `spawn_agent` and
`wait_agent`, with explicit file paths and a distinct output directory per account) — but that is the expensive downstream step, not part of fetching. Do not fan
labeling out to a whole roster without being asked.

## Requirements

- Node ≥ 20; the shared `analyze-x-mentions` labeler (`chunk.mjs`, `run-labels.mjs`, `merge-labels.mjs`,
  `clean.mjs`) and `render_charts.py` (`uv`, matplotlib); `$S/kol-spec.mjs` and `~/.local/state/intel/x/kols/vocab.json`.

## Tests

```
node --test "$SKILL_DIR/tests/test_profile_user.mjs" "$SKILL_DIR/tests/test_reps.mjs"
```

Pass `--apps <comma-separated archive slugs>` to reps.mjs for app sections; default is empty.
