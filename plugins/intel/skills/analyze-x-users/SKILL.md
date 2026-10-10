---
name: analyze-x-users
description: Profile the accounts behind an app's X/Twitter mentions — who talks about it (casual passers-by, social chatter, degen traders, referral promoters, own-token promoters, KOLs, giveaway farmers, critics), which die-hard daily promoters have a financial stake, and what motivates each segment — into a Chinese users doc with charts. Use when asked to profile / 画像 / 分类 the users or accounts mentioning an app, or who the "死粉" and shillers are. Needs an archive already labeled by analyze-x-mentions (labels.jsonl); NOT for what people say about the app (use analyze-x-mentions for reception.md).
---

# Analyze X users

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
commands use the scripts of the sibling `analyze-x-mentions` and `fetch-x-mentions` skills, so keep
the whole intel plugin installed. Commands run from any working directory and use these names:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
S="$SKILL_DIR/scripts"
T="$SKILL_DIR/../analyze-x-mentions/scripts"
STATE="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" state)"
OUT="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" output)"
```

`STATE` is the state root that holds the archives and `OUT` the output root for docs and charts.
Write paths in JSON arguments as absolute paths, with `~` expanded.

## Environment variables

Set these in `~/.config/intel/.env`, starting from the intel plugin's `.env.example`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; archives are read from `x/<slug>/` and the CRM is written to `x/crm.sqlite`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |
| `INTEL_OUTPUT_DIR` | Output root; docs and charts go under `x/<slug>/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

## Setup

The scripts need Node 20 or later, and `render_charts.py` needs `uv` (it installs matplotlib
itself) and the Arial Unicode CJK font, at `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Input and output

The skill reads `$STATE/x/<slug>/tweets.jsonl` and `$STATE/x/<slug>/labels.jsonl` as
`analyze-x-mentions` leaves them, with every clean post labeled with `about`, `sentiment`, `topic`
and `interest`. Run `analyze-x-mentions` first if `labels.jsonl` is missing or behind the archive.
It writes `$OUT/x/<slug>/users.md` and two charts, `$OUT/x/<slug>/images/<slug>-users-*.png`.
Everything else (`clean.json`, `authors.json`, `role_stats.json`, `reps/`) is scratch.

Do the motivation read and the adversarial review inline, yourself, against `reps/<role>.jsonl`
and `authors.json`; never spawn subagents for them, even when this skill itself runs inside a
subagent. The sample is at most 7 accounts × 35 posts per role, well under 1000 posts, so one
context reads it fastest and cheapest. If a corpus ever exceeds the available context, use the
Workflow tool in Claude Code; in Codex, report the size and agree on a separate bounded workflow.
Never spawn nested agents.

## Clean the posts

```sh
node "$T/clean.mjs" "$STATE/x/<slug>/tweets.jsonl" --out <scratch>/clean.json
```

## Profile the accounts

```sh
node "$S/profile-authors.mjs" <scratch>/clean.json --labels "$STATE/x/<slug>/labels.jsonl" --out <scratch> --team <official,handles,founder>
```

Pass every official handle and the founder in `--team`: only those accounts become `official` and
are kept out of the die-hard list and the representatives. For each account the script measures
posts, active days, span, engagement (likes + reposts + replies + quotes), inbound reach (the
distinct other accounts that @-mention it, used because `author_followers` is empty in the archive)
and the share of its posts labeled about the app, like, dislike, referral or paid promotion,
giveaway, trading results and own token. Each account gets one role, the first rule it matches:

| role | rule |
|---|---|
| official | in `--team` |
| giveaway | ≥ 30% giveaway posts, or ≥ 20 posts with zero engagement (bot) |
| promoter | ≥ 20% referral / paid-promotion posts, ≥ 3 posts |
| token-promoter | ≥ 30% own-token posts, ≥ 3 posts |
| kol | ≥ 5 posts and top 5% by average engagement or by inbound reach |
| trader | ≥ 40% trading-results posts |
| critic | ≥ 50% of about-the-app posts are dislikes, ≥ 3 of them |
| casual | ≤ 2 posts, none of the above |
| other | the rest: multi-posters with no dominant signal |

The script prints the share of accounts, posts and engagement per role, the one-post share, the KOL
thresholds, the official accounts' reach, the die-hards (≥ 50 posts on ≥ 30 days with ≥ 15%
referral) and seven representatives per role: the three most active, the two most engaging and
two from the middle. It writes `authors.json`, `role_stats.json` and `reps/<role>.jsonl`, where
each representative carries its features and up to 35 posts, the 20 most engaged plus 15 spread
over its timeline.

The rules are proxies over model labels, so a promoter with a 16% referral share lands in `other`
and a high-reach critic in `kol`. The doc reports the numbers and says who each label actually
caught.

## Read each segment's motivation

Write `<scratch>/app-facts.md` if `analyze-x-mentions` has not left one: the app, its official
handles (current and former) and founder, its referral and rewards mechanics, the noise common in
its mentions, and user slang.

Read `$SKILL_DIR/motivation-prompt.md` once, then follow it exactly for each `reps/<role>.jsonl`
except `official`, one role after another. For each role produce ranked motivations with quotes,
a money and affiliation count, what the accounts do otherwise, one line per account, and the
accounts the label misfits. The misfits describe who a bucket really is in its 分类 line; they get
no section of their own.

## Draw the charts

```sh
echo '{"out_dir":"x/<slug>/images","charts":[
  {"type":"grouped","file":"<slug>-users-segments.png","title":"各类用户占账号 / 推文 / 互动的比例（%）",
   "labels":["社交闲聊 / 蹭热度","普通用户 / 一次性提及",...],
   "series":[{"name":"%账号","values":[...],"color":"#2a78d6"},{"name":"%推文","values":[...],"color":"#1baf7a"},{"name":"%互动","values":[...],"color":"#eb6834"}]},
  {"type":"bar","file":"<slug>-users-diehard-promoters.png","title":"死忠 / 返佣推手（推文数，括号内为返佣占比）","labels":["@handle（52%）",...],"values":[...],"color":"#2a78d6"}
]}' | "$T/render_charts.py" /dev/stdin
```

Sort the roles in the segments chart by share of posts and take the values from `role_stats.json`
(`pa`, `pp`, `pe`). Open both PNGs and check that no labels collide.

## Write users.md

Write the doc in Chinese, concrete and from evidence only, in this shape:

```
# <App> 提及者用户画像（<domain>）

## 本质
**<one sentence: what this business really is and what keeps it alive; not what the app does>**
自我复制的机制，按重要性从上往下：
1. **<mechanism>**：<one line>   ← every mechanism the findings support, one line each, never a paragraph

## 一、先看这个（三个关键结论）
1–3. <the three findings a reader must leave with, each with its number>
![各类用户占账号 / 推文 / 互动的比例](images/<slug>-users-segments.png)

## 二、用户分成哪几类
| 类别 | %账号 | %推文 | %互动 | 一句话 |   ← every role, sorted by %推文, one plain-words line each
<one paragraph: which label is a mixed bag and who it actually caught, from the misfits>

## 三、谁是死忠 / 利益相关的每日推手
<the rule in one sentence; official accounts listed separately with their reach>
![死忠 / 返佣推手](images/<slug>-users-diehard-promoters.png)
| 账号 | 推文 | 活跃天 | 返佣占比 | 被@次数 | 画像 |   ← one row per die-hard, 画像 from the motivation agents' one-liners
**结论**：<are they fans or a business, with the referral-share range>

## 四、各类用户的动机（子智能体逐账号读出来的）
- **<role> → <motivation>**. <one or two lines> 
  > [@handle](https://x.com/<handle>/status/<id>)：*"<verbatim ≤ 25 words>"*
  (one bullet per role, 1–2 quotes each; casual and critic included)

## 五、总的动机图谱
<one sentence summary> then a numbered list of motivations ordered by influence, each one line

## 六、对我们的启示（做 <our product> 时）
- one line per lesson, each tied to a finding above

**TL;DR**
- at most 5 bullets, plain words
```

The doc reports on the app's users and states findings directly. It never mentions the pipeline
or your own work: no notes on removed bots, cleaned data or filtered fake accounts, no filter
thresholds and no "方法与可信度" section.

- Every @handle anywhere in the doc, in tables, prose and quote lines, is a link
  `[@handle](https://x.com/<handle>)`; a quote line's handle links to its post,
  `[@handle](https://x.com/<handle>/status/<id>)`.
- Quotes are verbatim and at most 25 words, a handful per section, never a link farm. Quote Chinese
  posts in Chinese.
- Every claim in 一 to 五 carries its number (share, count, referral %) or its quote.
- Call an account "paid" or "sponsored" only when a post says so; a referral code alone is "有返佣".

Before the review, check every `/status/<id>` in the doc against `clean.json`: the id exists and
the handle matches.

## Review the doc

Read `$SKILL_DIR/review-prompt.md` and follow it exactly, reviewing `$OUT/x/<slug>/users.md`
yourself against `<scratch>/app-facts.md`, `<scratch>/authors.json`, `<scratch>/role_stats.json`
and `<scratch>/reps/*.jsonl`: every `/status/<id>` (the id exists, the handle matches), every
number against the stats and every claim against a quote. Apply every MUST-FIX and SHOULD-FIX, and a NIT only when it is a
one-line change. Then return the paths to `users.md` and the two charts.

## Build the CRM

The CRM is a SQLite database of the accounts posting about several apps, with their profiles,
per-app roles and post counts, sample posts and a real-or-fake verdict. Build it from an analysis
directory that holds `crm/followers.json` and each slug's `<slug>/authors.json`:

```sh
python3 "$S/build-crm.py" --scratch <analysis-dir> --apps <comma-separated-archive-slugs>
```

It reads each archive from `$STATE/x/<slug>/`, whatever the working directory, and writes
`$STATE/x/crm.sqlite`.

## Tests

```sh
node --test "$SKILL_DIR/tests/test_profile_authors.mjs"
```
