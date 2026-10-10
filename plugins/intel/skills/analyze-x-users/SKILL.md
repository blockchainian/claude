---
name: analyze-x-users
description: Profile the accounts behind an app's X/Twitter mentions — who talks about it (casual passers-by, social chatter, degen traders, referral promoters, own-token promoters, KOLs, giveaway farmers, critics), which die-hard daily promoters have a financial stake, and what motivates each segment — into a Chinese users doc with charts. Use when asked to profile / 画像 / 分类 the users or accounts mentioning an app, or who the "死粉" and shillers are. Needs an archive already labeled by analyze-x-mentions (labels.jsonl); NOT for what people say about the app (use analyze-x-mentions for reception.md).
---

# Analyze Twitter Users

## Runtime and paths

These skills run in Claude Code and Codex. Set `SKILL_DIR` to the absolute directory of this
loaded `SKILL.md`, not the working directory or a host environment variable, and repeat it, with
any other variable a command below uses, in every shell call:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

If the loaded path is unavailable, stop and report it. Keep the whole intel plugin installed,
because sibling skills share scripts.

For a long-running command, choose a deadline before launch and keep the process handle and its
output. In Claude Code run it with `run_in_background` and wait for the completion notification;
in Codex keep the shell tool's session handle and wait on it. A subagent waits for its own commands
before returning. Do not poll logs in a loop. On a timeout, keep the diagnostics and report the
process state before retrying.

Archive paths below show the default `~/.local/state/intel` root; replace it with the configured `INTEL_STATE_DIR` when set. Docs and charts go under `x/<slug>/` in the output root, which the commands below read into `OUT`. Expand `~` to the absolute home path in JSON arguments.

## Environment variables

Set these in `~/.config/intel/.env`, starting from the intel plugin’s `.env.example`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_OUTPUT_DIR` | Output root; docs and charts go under `x/<slug>/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

The accounts behind one app's X mentions → `users.md`: how the accounts split into
behavioral segments, how much of the posts and engagement each segment owns, the
die-hard daily promoters and whether they have a stake, and each segment's
motivation read from their own posts. Numbers come from the whole corpus; motivations
are read inline from each segment's representative accounts; an adversarial
self-review checks quotes and overclaims before the doc ships.

## Input & output

- **Input**: `~/.local/state/intel/x/<slug>/tweets.jsonl` and `~/.local/state/intel/x/<slug>/labels.jsonl`
  as left by `analyze-x-mentions` (every clean post labeled: `about, sentiment, topic, interest`).
  Run `analyze-x-mentions` first if `labels.jsonl` is missing or behind the archive.
- **Output**: `$OUT/x/<slug>/users.md` and two charts in
  `$OUT/x/<slug>/images/<slug>-users-*.png`.

Commands may run from any working directory; `S="$SKILL_DIR/scripts"`,
`T="$SKILL_DIR/../analyze-x-mentions/scripts"` and
`OUT="$(node "$SKILL_DIR/../fetch-x-mentions/scripts/env.mjs" output)"` below.

> **Run steps 3 and 6 inline — do not spawn subagents for them.** The motivation read
> and the adversarial review are done by whoever runs this skill, directly against
> `reps/<role>.jsonl` and `authors.json`. The representative sample is small (≤ 7
> accounts × ≤ 35 posts per role, well under 1000 posts total), so one context reads it
> fast and far cheaper than a fan-out. This rule applies in either host, including
> when the skill runs inside a subagent. If a future corpus exceeds the available
> context, use Claude Code's Workflow tool there; in Codex report the size and
> agree on a separate bounded workflow. Never spawn nested agents.

## Procedure

### 1. Clean (analyze-x-mentions script)

```
node $T/clean.mjs ~/.local/state/intel/x/<slug>/tweets.jsonl --out <scratch>/clean.json
```

### 2. Profile the accounts (script)

```
node $S/profile-authors.mjs <scratch>/clean.json --labels ~/.local/state/intel/x/<slug>/labels.jsonl --out <scratch> --team <official,handles,founder>
```

Per account: posts, active days, span, engagement (likes + reposts + replies + quotes),
inbound reach (distinct other accounts that @-mention it; `author_followers` is empty in the
archive), and label shares: about the app, like, dislike, referral / paid promotion,
giveaway, trading results, own token. One role per account, first match wins:

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

Prints the role table (share of accounts / posts / engagement), the one-post share, the KOL
thresholds, the official accounts' reach, the die-hards (≥ 50 posts on ≥ 30 days with
≥ 15% referral; the founder and brand accounts excluded), and the seven representatives
per role (three most active, two most engaging, two from the middle). Writes
`authors.json`, `role_stats.json`, and `reps/<role>.jsonl`: each representative with its
features and up to 35 posts (the 20 most engaged plus 15 spread over its timeline, so the
sample is not only the viral promotional posts). The rules are proxies over model labels:
a promoter with a 16% referral share lands in `other`, a high-reach critic in `kol`; the
doc reports the numbers and says who the label actually caught.

### 3. Motivation, read inline per role

Write `<scratch>/app-facts.md` if `analyze-x-mentions` has not left one: the app, its official
handles (current and former) and founder, its referral / rewards mechanics, the noise
common in its mentions, user slang.

Then read `$SKILL_DIR/motivation-prompt.md` once and, following it
exactly, work through each `reps/<role>.jsonl` yourself (skip `official`) — inline, in this
context, not by spawning a subagent. For each role produce: ranked motivations with quotes,
a money / affiliation count, what the accounts do otherwise, one line per account, and which
accounts the label misfits. Use the misfits to describe who a bucket really is in its 分类 line (not as a separate section). The
whole sample is small (≤ 7 accounts × ≤ 35 posts per role), so reading all roles in one pass
is fast; do the roles one after another rather than fanning out.

### 4. Charts (analyze-x-mentions script)

```
echo '{"out_dir":"x/<slug>/images","charts":[
  {"type":"grouped","file":"<slug>-users-segments.png","title":"各类用户占账号 / 推文 / 互动的比例（%）",
   "labels":["社交闲聊 / 蹭热度","普通用户 / 一次性提及",...],
   "series":[{"name":"%账号","values":[...],"color":"#2a78d6"},{"name":"%推文","values":[...],"color":"#1baf7a"},{"name":"%互动","values":[...],"color":"#eb6834"}]},
  {"type":"bar","file":"<slug>-users-diehard-promoters.png","title":"死忠 / 返佣推手（推文数，括号内为返佣占比）","labels":["@handle（52%）",...],"values":[...],"color":"#2a78d6"}
]}' | $T/render_charts.py /dev/stdin
```

Roles in the segments chart sorted by share of posts; values from `role_stats.json`
(`pa`, `pp`, `pe`). Open both PNGs and check for label collisions.

### 5. Write `users.md` (Chinese, concrete, evidence only)

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

Rules:
- This is a report on the app's users, not a lab notebook. Never mention the pipeline or your own
  work: no "removed bots / cleaned data / filtered fake accounts" notes, no filter thresholds, no
  "方法与可信度" section. The reader wants what the data says about the users, not what you did to
  get it. State findings directly.
- Every @handle anywhere in the doc — die-hard tables, prose, the 动机 quote lines — is a clickable
  link `[@handle](https://x.com/<handle>)`, so the reader can open the account. (A quote line's
  own handle is `[@handle](https://x.com/<handle>/status/<id>)`, linking to the specific post.)
- Quotes are verbatim, ≤ 25 words; a handful per section, never a link farm. Chinese posts quoted in Chinese.
- Every claim in 一–五 carries its number (share, count, referral %) or its quote. An account
  is "paid" or "sponsored" only when a post says so; a referral code alone is "有返佣".
- Before the review, check every `/status/<id>` in the doc against `clean.json` (id exists,
  handle matches).

### 6. Adversarial review, inline

Read `$SKILL_DIR/review-prompt.md` and follow it exactly, reviewing the
doc yourself — inline, not by spawning a subagent. Check `$OUT/x/<slug>/users.md`
against `<scratch>/app-facts.md`, `<scratch>/authors.json`, `<scratch>/role_stats.json` and
`<scratch>/reps/*.jsonl`: verify every `/status/<id>` (id exists, handle matches), every
number against the stats, and every claim against a quote. Apply every MUST-FIX and
SHOULD-FIX; a NIT only when it is a one-line change.

### 7. Ship

Return the paths to `users.md` and the two images under the output root. `clean.json`, `authors.json`,
`role_stats.json` and `reps/` are scratch.

## Requirements

- Node ≥ 20; `uv` for `render_charts.py` (matplotlib); CJK font at
  `/System/Library/Fonts/Supplemental/Arial Unicode.ttf`.

## Tests

```
node --test "$SKILL_DIR/tests/test_profile_authors.mjs"
```

Archives are read from `INTEL_STATE_DIR/x/`, independent of the working directory.

Build the CRM from the owning archive root (cwd) with explicit inputs:
```sh
python3 "$SKILL_DIR/scripts/build-crm.py" --scratch <analysis-dir> --apps <comma-separated-archive-slugs>
```
The analysis directory contains crm/followers.json and each slug's authors.json.
Output is `INTEL_STATE_DIR/x/crm.sqlite`, default `~/.local/state/intel/x/crm.sqlite`.
