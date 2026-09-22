---
name: analyze-appstore-reviews
description: Turn a scraped App Store reviews JSON into a concise, data-driven Chinese analysis doc plus charts — most-liked and most-disliked patterns ranked by frequency, and the top feature requests. Use when asked to analyze/分析 an app's App Store reviews, summarize what users love and hate, or extract feature requests from a reviews dataset. NOT for scraping reviews (the JSON must already exist) or for non-review market research.
---

# Analyze App Store Reviews

Turn one app's scraped reviews into a short, **evidence-only** analysis: what users
love, what they hate, and what they ask for — each ranked by how often it actually
appears, every claim backed by the data. The output reads in one pass and hides
nothing.

**以数据说话，不瞎编。** Every number comes from the JSON; every quote is verbatim.
Counts rank themes — call them 量级, not precise values. When you cannot support a
claim from the data, drop it.

## Input & output

- **Input**: a reviews JSON whose `reviews[]` items carry `rating` (1–5), `title`,
  `body`, `date`, `country`, and optionally `developerResponseBody`. This is the
  App Store scraper's shape.
- **Output**, next to the input as `<app>/analysis.md` + `<app>/images/`:
  - `analysis.md` — concise **Chinese** doc (structure below).
  - four charts — rating distribution, likes, dislikes, top feature requests.

## Procedure

### 1. Ground with deterministic stats (script)

```
"${CLAUDE_PLUGIN_ROOT}/skills/analyze-appstore-reviews/scripts/stats.py" \
  <reviews.json> --dump-dir <scratch>
```

Prints a stats JSON (total, 1–5★ distribution, avg, US vs non-US, date range,
developer-response count, `n_dislike_1_3`, `n_like_4_5`, top countries) and writes
`neg.txt` (1–2★), `mid.txt` (3★), `pos.txt` (4–5★). These numbers are facts — use
them as-is.

### 2. Read ALL the text, split by sentiment

Read the whole of `neg.txt`, `mid.txt`, `pos.txt`. **Do not sample.** Reading every
review is what grounds themes in what people actually said, instead of guessing from
keywords. For a large dataset, delegate the reading to a subagent and keep only the
themes.

### 3. Count themes with review-level keyword hits

For each theme you saw, write a regex and count **reviews that match** (one hit per
review). Count like-themes only within **4–5★**, dislike-themes only within
**1–3★** ("easy to deposit, impossible to withdraw" is not praise). Discipline:

- Word-boundary the patterns. Spot-check every bucket by printing ~10 sample
  matches before trusting its count — kill false positives (`down`→download,
  `fun`→fund, `card`→credit card, `hot`→shot, `ban`→bank, `tail`→retail).
- The counts only **rank**; present them as 量级.

### 4. Derive the top feature requests

Count only **explicit product asks** ("please add / wish / missing X"), separate
from complaints. Note the semantics (e.g. users who already copy-trade asking for
*auto* execution). **If complaint themes — lower fees, fix speed — would outrank the
feature list, say so in one line rather than hide it.**

### 5. Write `analysis.md` (concise Chinese)

Sections:

- A short scope + method note. **No data-source citation** — no JSON path, no appId,
  no "可复算/可核对". The reader knows where the data is from.
- **一、这批数据是什么** — rating distribution + a one-line "what is this app".
- **二、最喜欢什么（4–5★）** — themes ranked, each `· <count>`.
- **三、最不喜欢什么（1–3★）** — themes ranked, each `· <count>`. When a "scam"
  bucket dominates, split it into (a) real operational failures (deposit taken but
  not credited, can't withdraw) vs (b) memecoin/asset loss blamed on the app; flag
  platform-level allegations (freeze-and-dump, wash trading) as **未证实 user
  claims**, not facts.
- **四、Top 5 功能请求 / 改进建议** — with the fee/speed caveat from step 4.
- **五、数据质量与方法** — the noise the reader must know: star-vs-text mismatch
  (5★ that say "scam"/"trash" for visibility; 1★ that say "good"), promo/referral-code
  reviews inflating praise, low-information shill short reviews, non-English
  undercount by English regexes, keyword counts are 量级 not precise, survivorship
  bias toward the two extremes.

**Quotes**: blockquote the **comment body only**, plain text, verbatim — **no star,
no title, no id, no country, no bold, no italics, no brackets**. One `>` line per
quote. Keep the prose tight; every line costs the reader.

### 6. Charts (script)

Build a spec and render. Chinese labels; one horizontal bar chart per ranked section
(likes `#1baf7a`, dislikes `#eb6834`, requests `#2a78d6`) plus a rating chart:

```
echo '{"out_dir":"<app>/images","charts":[
  {"type":"rating","file":"<app>-rating-distribution.png","title":"评分分布：两极分化","values":[C1,C2,C3,C4,C5]},
  {"type":"bar","file":"<app>-likes.png","title":"最喜欢什么（4–5★）","labels":[...],"values":[...],"color":"#1baf7a"},
  {"type":"bar","file":"<app>-dislikes.png","title":"最不喜欢什么（1–3★）","labels":[...],"values":[...],"color":"#eb6834"},
  {"type":"bar","file":"<app>-feature-requests.png","title":"Top 5 功能请求","labels":[...],"values":[...],"color":"#2a78d6"}
]}' | "${CLAUDE_PLUGIN_ROOT}/skills/analyze-appstore-reviews/scripts/render_charts.py" /dev/stdin
```

Charts are transparent with dual-mode gray text (work in dark and light mode) and
carry a title only — no subtitle, legend, or stat line. Embed each at the top of its
section: `![最喜欢什么](images/<app>-likes.png)`.

### 7. Adversarial review before shipping

Spawn an adversarial subagent: independently recompute the distribution and every
theme count with its own method, verify each quote exists and is accurate, check the
top-5 ordering, and hunt cherry-picked quotes and star-vs-text contamination. Apply
the valid findings to `analysis.md`; then tell the user what it actually caught.

### 8. Ship

Commit, push, and open the doc + charts for the user.

## Requirements

- `uv` on PATH (both scripts declare their own deps; `render_charts.py` pulls
  matplotlib).
- A CJK font — the renderer tries Arial Unicode / Hiragino Sans GB / STHeiti.
- Style sibling for tone and structure: an existing `reception.md`-style analysis if
  the repo has one.
