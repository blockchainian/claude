---
name: analyze-appstore-reviews
description: Turn a scraped App Store reviews JSON into a concise, data-driven Chinese analysis doc plus charts — most-liked and most-disliked patterns ranked by frequency, and the top feature requests. Use when asked to analyze/分析 an app's App Store reviews, summarize what users love and hate, or extract feature requests from a reviews dataset. NOT for scraping reviews (the JSON must already exist) or for non-review market research.
---

# Analyze App Store reviews

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Set these in `~/.config/intel/.env`, starting from the intel plugin’s `.env.example`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_OUTPUT_DIR` | Output root; analyses go under `reviews/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

## Setup

`stats.mjs` needs Node.js 18.18+ and no npm packages. `render_charts.py` needs `uv` on PATH, which
pulls matplotlib itself, and a CJK font; it uses Arial Unicode, Hiragino Sans GB or STHeiti,
whichever it finds first.

## Input and output

The input is a reviews JSON in the App Store scraper's shape: a `reviews[]` array whose items carry
`rating` (1–5), `title`, `body`, `date`, `country` and optionally `developerResponseBody`. The
output goes to `reviews/<app>/` under the output root, named after the reviews JSON, and the stats
JSON's `outDir` gives its absolute path. It holds `analysis.md`, a concise Chinese doc, and an
`images/` folder with four charts: the rating distribution, likes, dislikes and top feature
requests.

The analysis is evidence only. **以数据说话，不瞎编。** Every number comes from the JSON and every
quote is verbatim; when the data cannot support a claim, drop it. Counts only rank themes, so
present them as 量级, not precise values.

## Ground the numbers

```sh
"$SKILL_DIR/scripts/stats.mjs" <reviews.json> --dump-dir <scratch>
```

The script prints a stats JSON with `outDir`, the total, the 1–5★ distribution, the average, US
versus non-US, the date range, the developer-response count, `n_dislike_1_3`, `n_like_4_5` and the
top countries. It also writes `neg.txt` (1–2★), `mid.txt` (3★) and `pos.txt` (4–5★) to the dump
directory. These numbers are facts; use them as they are.

## Read every review

Read the whole of `neg.txt`, `mid.txt` and `pos.txt`. **Do not sample**: reading every review is
what grounds the themes in what people actually said rather than in guessed keywords. For a large
dataset, hand the reading to a subagent that starts with no forked history (in Codex,
`spawn_agent` with `fork_turns: "none"`), give it the input paths
and the reading brief, and keep only the themes it returns; collect its result before counting.

## Count themes

For each theme you saw, write a regex and count the **reviews that match**, one hit per review.
Count like-themes only within **4–5★** and dislike-themes only within **1–3★**, because "easy to
deposit, impossible to withdraw" is not praise.

Bound each term with `(?<![A-Za-z0-9])term(?![A-Za-z0-9])`, not `\b`: Python's `\w` includes CJK,
so `\bapp\b` misses `手机app`. Before trusting a bucket's count, print about 10 sample matches and
remove false positives such as `down`→download, `fun`→fund, `card`→credit card, `hot`→shot,
`ban`→bank and `tail`→retail.

## Find the top feature requests

Count only **explicit product asks** ("please add", "wish", "missing X"), kept apart from
complaints, and note what the ask really means, for example users who already copy-trade asking for
*auto* execution. **If complaint themes such as lower fees or faster speed would outrank the feature
list, say so in one line rather than hide it.**

## Write the analysis

Write `analysis.md` in concise Chinese. Open with a short scope and method note that carries **no
data-source citation**: no JSON path, no appId, no "可复算/可核对"; the reader knows where the data
comes from. Then write these sections:

- **一、这批数据是什么**: the rating distribution and a one-line "what is this app".
- **二、最喜欢什么（4–5★）**: themes ranked, each `· <count>`.
- **三、最不喜欢什么（1–3★）**: themes ranked, each `· <count>`. When a "scam" bucket dominates,
  split it into (a) real operational failures, such as a deposit taken but not credited or a
  withdrawal that fails, and (b) memecoin or asset losses blamed on the app. Flag platform-level
  allegations such as freeze-and-dump or wash trading as **未证实 user claims**, not facts.
- **四、Top 5 功能请求 / 改进建议**: with the fee and speed caveat from the feature requests.
- **五、数据质量与方法**: the noise the reader must know about: star-versus-text mismatch (5★ reviews
  that say "scam" or "trash" for visibility, 1★ reviews that say "good"), promo and referral-code
  reviews inflating praise, low-information shill short reviews, non-English reviews undercounted
  by English regexes, keyword counts being 量级 rather than precise, and survivorship bias toward
  the two extremes.

Quote **the comment body only**, verbatim and as plain text, one `>` line per quote, with **no
star, title, id, country, bold, italics or brackets**. Keep the prose tight; every line costs the
reader. If the repo already holds a `reception.md`-style analysis, follow its tone and structure.

## Draw the charts

Build a spec with Chinese labels, one horizontal bar chart per ranked section in its fixed colour
(likes `#1baf7a`, dislikes `#eb6834`, requests `#2a78d6`) plus the rating chart, and list each
chart's labels and values in ranked order, largest first:

```
echo '{"out_dir":"<outDir>/images","charts":[
  {"type":"rating","file":"<app>-rating-distribution.png","title":"评分分布：两极分化","values":[C1,C2,C3,C4,C5]},
  {"type":"bar","file":"<app>-likes.png","title":"最喜欢什么（4–5★）","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<app>-dislikes.png","title":"最不喜欢什么（1–3★）","labels":[...],"values":[...],"color":"#eb6834"},
  {"type":"bar","file":"<app>-feature-requests.png","title":"Top 5 功能请求","labels":[...],"values":[...],"color":"#2a78d6"}
]}' | "$SKILL_DIR/scripts/render_charts.py" /dev/stdin
```

Embed each chart at the top of its section, for example `![最喜欢什么](images/<app>-likes.png)`.

## Review adversarially

Before shipping, spawn an adversarial subagent of the same kind and give it the
artifact paths with this brief: independently recompute the distribution and every theme count with
its own method, verify that each quote exists and is accurate, check the top-5 ordering, and hunt
for cherry-picked quotes and star-versus-text contamination. Wait for its final result, apply the
valid findings to `analysis.md`, and tell the user what it actually caught.

## Ship

Commit, push, and open the doc and charts for the user.
