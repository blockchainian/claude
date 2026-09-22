#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.13"
# dependencies = ["numpy", "torch", "sentence-transformers", "transformers", "bertopic", "hdbscan", "umap-learn", "scikit-learn"]
# ///
# ABOUTME: Finds hot topics and sentiment over the whole clean corpus without a keyword list: bge-small
# ABOUTME: embeddings + BERTopic clusters + a tweet sentiment model, cached by tweet id so reruns only see new posts.
#
# Usage: topics.py <clean.json> --cache <dir> --out <dir> [--refit] [--since YYYY-MM-DD] [--until YYYY-MM-DD]
#        [--names <file>] [--sample 100000] [--min-topic-size 100] [--per-day 3]
#   clean.json  the whole archive from clean.mjs (no --since/--until there; the window is applied here)
#   --cache     per-slug directory with embeddings, cluster ids, sentiment and the cluster model (rebuildable)
#   --names     the topics file, {"<id>": {"name", "keywords"}}; the one file worth committing
#   --out       where labels0.json (for aggregate.mjs) and clusters.json (unnamed clusters to name) land
#   --refit     re-cluster from scratch on a sample of the cached embeddings, then assign every post;
#               clusters whose keywords overlap a named old cluster keep its name
import argparse
import json
import re
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import numpy as np

EMBEDDING_MODEL = "BAAI/bge-small-en-v1.5"
SENTIMENT_MODEL = "cardiffnlp/twitter-roberta-base-sentiment-latest"
UNASSIGNED = -2  # cached but not yet run through the cluster model
UNCLUSTERED = "无话题"  # HDBSCAN outliers (over half of all tweets); pushing them into the nearest cluster was only 40% right
OTHER_LANG = "其他语言"
NOISE = "噪音"  # the name for spam and banter clusters; they never count as "about" the app
SENTIMENT = {"negative": "dislike", "neutral": "neutral", "positive": "like"}


def day_of(t):
    s = t["created_at"]
    try:
        d = datetime.strptime(s, "%a %b %d %H:%M:%S %z %Y")
    except ValueError:
        d = datetime.fromisoformat(s.replace("Z", "+00:00"))
    return d.astimezone(timezone.utc).strftime("%Y-%m-%d")


def in_window(day, since, until):
    return (since is None or day >= since) and (until is None or day < until)


def is_english(t):
    return t.get("lang") == "en"


def clean_text(text):
    return re.sub(r"\s+", " ", re.sub(r"https?:\S+", "", text)).strip()


class Cache:
    """Per-tweet text, embedding, sentiment and cluster id aligned by position in ids, plus the cluster keywords and names."""

    def __init__(self, dir):
        self.dir = Path(dir)
        self.dir.mkdir(parents=True, exist_ok=True)
        self.ids = self._json("ids.json", [])
        self.sentiment = self._json("sentiment.json", [])
        self.docs = self._json("docs.json", [])
        self.keywords = self._json("keywords.json", {})
        self.names = {}  # cluster id → 中文名, loaded from the names file by the caller
        n = len(self.ids)
        self.emb = np.load(self.dir / "emb.npy") if n else np.zeros((0, 0), dtype=np.float16)
        self.topic = np.load(self.dir / "topic.npy") if n else np.zeros(0, dtype=np.int32)
        self.index = {id: i for i, id in enumerate(self.ids)}

    def _json(self, name, default):
        p = self.dir / name
        return json.loads(p.read_text()) if p.exists() else default

    @property
    def model_path(self):
        return self.dir / "model.pkl"

    def append(self, ids, docs, emb, sentiment):
        self.emb = emb.astype(np.float16) if not self.ids else np.vstack([self.emb, emb.astype(np.float16)])
        self.topic = np.concatenate([self.topic, np.full(len(ids), UNASSIGNED, dtype=np.int32)])
        self.ids.extend(ids)
        self.docs.extend(docs)
        self.sentiment.extend(sentiment)
        self.index = {id: i for i, id in enumerate(self.ids)}

    def save(self):
        np.save(self.dir / "emb.npy", self.emb)
        np.save(self.dir / "topic.npy", self.topic)
        for name, v in [("ids.json", self.ids), ("docs.json", self.docs), ("sentiment.json", self.sentiment),
                        ("keywords.json", self.keywords)]:
            (self.dir / name).write_text(json.dumps(v))

    def label(self, tid):
        key = str(tid)
        return self.names.get(key) or ", ".join(self.keywords.get(key, [])[:3]) or key


def read_names(path):
    """The committed names file: {"<cluster id>": {"name": "<中文名>", "keywords": [...]}} → {id: name}."""
    p = Path(path)
    return {k: v["name"] for k, v in json.loads(p.read_text()).items()} if p.exists() else {}


def write_names(path, cache):
    Path(path).write_text(json.dumps({k: {"name": cache.names[k], "keywords": cache.keywords.get(k, [])}
                                      for k in sorted(cache.names, key=int)}, ensure_ascii=False, indent=1) + "\n")


def inherit_names(old, keywords, min_overlap=0.5):
    """After a refit the cluster ids change; a new cluster takes the name of the old one whose keywords
    it shares most (Jaccard ≥ min_overlap). old: {id: {name, keywords}} from the names file."""
    names = {}
    for key, kws in keywords.items():
        best, score = None, 0
        for o in old.values():
            a, b = set(kws), set(o["keywords"])
            j = len(a & b) / len(a | b) if a | b else 0
            if j > score:
                best, score = o["name"], j
        if best and score >= min_overlap:
            names[key] = best
    return names


def update_cache(tweets, cache, encode, sentiment, lap=lambda msg: None):
    """Embed and score the english tweets not yet cached; returns how many were added."""
    new = [t for t in tweets if is_english(t) and t["id"] not in cache.index]
    if not new:
        return 0
    texts = [clean_text(t["text"]) for t in new]
    emb = encode(texts)
    lap(f"embedded {len(new)} new posts")
    cache.append([t["id"] for t in new], texts, emb, sentiment(texts))
    return len(new)


def refit(cache, clusterer, sample):
    rng = np.random.default_rng(42)
    n = len(cache.ids)
    pick = np.sort(rng.choice(n, size=min(sample, n), replace=False))
    docs = cache.docs
    clusterer.fit([docs[i] for i in pick], cache.emb[pick].astype(np.float32))
    cache.keywords = {str(k): v for k, v in clusterer.keywords.items()}
    cache.topic = clusterer.transform(docs, cache.emb.astype(np.float32)).astype(np.int32)


def assign_new(cache, clusterer):
    """Cluster id per new post; -1 (outlier) stays -1 and counts as 无话题. The outlier share of the new
    posts, against the all-time share, is the signal that a refit is due."""
    idx = np.flatnonzero(cache.topic == UNASSIGNED)
    if len(idx):
        cache.topic[idx] = clusterer.transform([cache.docs[i] for i in idx], cache.emb[idx].astype(np.float32))
    return len(idx)


def _cluster_key(t, cache):
    """OTHER_LANG, UNCLUSTERED, or the cluster id as a string."""
    if not is_english(t):
        return OTHER_LANG
    i = cache.index.get(t["id"])
    if i is None or cache.topic[i] < 0:
        return UNCLUSTERED
    return str(int(cache.topic[i]))


def _cached_topic(t, cache):
    key = _cluster_key(t, cache)
    return key if key in (OTHER_LANG, UNCLUSTERED) else cache.label(int(key))


def _cached_sentiment(t, cache):
    i = cache.index.get(t["id"])
    return cache.sentiment[i] if i is not None else "neutral"


def table(tweets, cache, since=None, until=None):
    """One row per topic label; clusters that share a name are one topic."""
    by_topic, keys = defaultdict(list), defaultdict(list)
    for t in tweets:
        if in_window(day_of(t), since, until):
            key = _cluster_key(t, cache)
            topic = _cached_topic(t, cache)
            by_topic[topic].append(t)
            if key not in keys[topic] and key not in (OTHER_LANG, UNCLUSTERED):
                keys[topic].append(key)
    rows = []
    for topic, hits in by_topic.items():
        by_day = defaultdict(int)
        for t in hits:
            by_day[day_of(t)] += 1
        peak = min(by_day.items(), key=lambda kv: (-kv[1], kv[0]))  # ties go to the earliest day
        kws = []
        for key in keys[topic]:
            kws += [w for w in cache.keywords.get(key, [])[:5] if w not in kws]
        rows.append({"topic": topic, "n": len(hits), "authors": len({t["author"] for t in hits}),
                     "likes": sum(t.get("likes") or 0 for t in hits), "peakDay": peak[0], "peakN": peak[1],
                     "keywords": ", ".join(kws[:8])})
    return sorted(rows, key=lambda r: (-r["n"], -r["likes"], r["topic"]))


def timeline(tweets, per_day=3, since=None, until=None):
    by_day = defaultdict(list)
    for t in tweets:
        d = day_of(t)
        if in_window(d, since, until):
            by_day[d].append(t)
    return [{"day": d, "n": len(v), "top": sorted(v, key=lambda t: -(t.get("likes") or 0))[:per_day]}
            for d, v in sorted(by_day.items())]


def labels(tweets, cache, since=None, until=None):
    return [{"id": t["id"], "about": is_english(t) and _cached_topic(t, cache) not in (UNCLUSTERED, OTHER_LANG, NOISE),
             "sentiment": _cached_sentiment(t, cache) if is_english(t) else "neutral",
             "topic": _cached_topic(t, cache)} for t in tweets if in_window(day_of(t), since, until)]


def clusters_for_naming(tweets, cache, k=10):
    """Keywords and the k most-liked posts of every cluster that has no Chinese name yet."""
    by_id = {t["id"]: t for t in tweets}
    members = defaultdict(list)
    for id, tid in zip(cache.ids, cache.topic):
        if tid >= 0 and str(tid) not in cache.names and id in by_id:
            members[str(tid)].append(by_id[id])
    out = {}
    for key in sorted(members, key=int):
        top = sorted(members[key], key=lambda t: -(t.get("likes") or 0))[:k]
        out[key] = {"keywords": cache.keywords.get(key, []), "n": len(members[key]),
                    "examples": [{"id": t["id"], "likes": t.get("likes") or 0, "text": clean_text(t["text"])} for t in top]}
    return out


# ---- real models (imported lazily so the tests and the hot path stay light) ----

def device():
    import torch
    return "mps" if torch.backends.mps.is_available() else "cuda" if torch.cuda.is_available() else "cpu"


def make_encoder():
    import torch
    from sentence_transformers import SentenceTransformer
    m = SentenceTransformer(EMBEDDING_MODEL, device=device(), model_kwargs={"torch_dtype": torch.float16})
    m.max_seq_length = 128
    return lambda texts: m.encode(texts, batch_size=256, normalize_embeddings=True, convert_to_numpy=True, show_progress_bar=False)


def make_sentiment():
    import torch
    from transformers import pipeline
    clf = pipeline("sentiment-analysis", model=SENTIMENT_MODEL, device=device(), truncation=True, max_length=128,
                   dtype=torch.float16)

    def run(texts):
        # the model was trained with these placeholders; length-sorted batches pad far less (3× faster)
        prepped = [re.sub(r"@\w+", "@user", t) for t in texts]
        order = sorted(range(len(prepped)), key=lambda i: len(prepped[i]))
        out = [None] * len(prepped)
        for i, r in zip(order, clf([prepped[i] for i in order], batch_size=256)):
            out[i] = SENTIMENT[r["label"]]
        return out
    return run


class BertopicClusterer:
    def __init__(self, min_topic_size):
        self.min_topic_size = min_topic_size
        self.model = None

    @classmethod
    def load(cls, path):
        from bertopic import BERTopic
        c = cls(0)
        c.model = BERTopic.load(str(path))
        return c

    def fit(self, docs, emb):
        from bertopic import BERTopic
        from hdbscan import HDBSCAN
        from sklearn.feature_extraction.text import CountVectorizer
        from umap import UMAP
        self.model = BERTopic(
            embedding_model=None,
            umap_model=UMAP(n_neighbors=15, n_components=5, min_dist=0.0, metric="cosine", random_state=42),
            # min_samples well below the cluster size: the default (= cluster size) leaves most tweets unclustered
            hdbscan_model=HDBSCAN(min_cluster_size=self.min_topic_size, min_samples=max(5, self.min_topic_size // 10),
                                  metric="euclidean", prediction_data=True),
            vectorizer_model=CountVectorizer(stop_words="english", min_df=5, token_pattern=r"(?u)\b[a-zA-Z][a-zA-Z0-9_]+\b"),
            calculate_probabilities=False, verbose=False)
        topics, _ = self.model.fit_transform([re.sub(r"@\w+", "", d) for d in docs], emb)
        return np.array(topics)

    def transform(self, docs, emb):
        topics, _ = self.model.transform([re.sub(r"@\w+", "", d) for d in docs], emb)
        return np.array(topics)

    @property
    def keywords(self):
        return {tid: [w for w, _ in self.model.get_topic(tid)][:10] for tid in self.model.get_topics() if tid != -1}

    def save(self, path):
        self.model.save(str(path), serialization="pickle")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("clean")
    ap.add_argument("--cache", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--names", help="the topics file, default <cache>/topics.json; commit it next to the doc")
    ap.add_argument("--refit", action="store_true")
    ap.add_argument("--since")
    ap.add_argument("--until")
    ap.add_argument("--sample", type=int, default=100_000)
    ap.add_argument("--min-topic-size", type=int, default=100)
    ap.add_argument("--per-day", type=int, default=3)
    a = ap.parse_args()

    t0 = time.time()
    lap = lambda msg: print(f"[{time.time() - t0:6.1f}s] {msg}", file=sys.stderr)
    tweets = json.loads(Path(a.clean).read_text())
    cache = Cache(a.cache)
    names_path = a.names or cache.dir / "topics.json"
    old_names = json.loads(Path(names_path).read_text()) if Path(names_path).exists() else {}
    cache.names = {k: v["name"] for k, v in old_names.items()}
    lap(f"{len(tweets)} posts, {len(cache.ids)} cached, {len(cache.names)} named clusters")

    pending = [t for t in tweets if is_english(t) and t["id"] not in cache.index]
    new = update_cache(pending, cache, make_encoder(), make_sentiment(), lap) if pending else 0
    lap(f"scored {new} new posts")
    if a.refit or not cache.model_path.exists():
        cl = BertopicClusterer(a.min_topic_size)
        refit(cache, cl, a.sample)
        cl.save(cache.model_path)
        cache.names = inherit_names(old_names, cache.keywords)
        lap(f"refit on {min(a.sample, len(cache.ids))} posts: {len(cache.keywords)} clusters, {len(cache.names)} names inherited")
    elif new:
        n = assign_new(cache, BertopicClusterer.load(cache.model_path))
        lap(f"assigned {n} new posts")
    cache.save()
    write_names(names_path, cache)

    if new:
        lap(f"outlier share of new posts {100 * (cache.topic[-new:] == -1).mean():.0f}% (all cached {100 * (cache.topic == -1).mean():.0f}%); rising → --refit")

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "labels0.json").write_text(json.dumps(labels(tweets, cache, a.since, a.until)))
    unnamed = clusters_for_naming(tweets, cache)
    (out / "clusters.json").write_text(json.dumps(unnamed, ensure_ascii=False, indent=1))
    lap(f"{len(unnamed)} clusters without a name → {out / 'clusters.json'}; add them to {names_path} and rerun")

    rows = table(tweets, cache, a.since, a.until)
    w = max(len(r["topic"]) for r in rows) if rows else 5
    print(f"{'topic':<{w}}  {'n':>6} {'authors':>7} {'likes':>7}  peakDay     peakN  keywords")
    for r in rows:
        print(f"{r['topic']:<{w}}  {r['n']:>6} {r['authors']:>7} {r['likes']:>7}  {r['peakDay']}  {r['peakN']:>5}  {r['keywords']}")
    for d in timeline(tweets, a.per_day, a.since, a.until):
        print(f"\n## {d['day']} ({d['n']})")
        for t in d["top"]:
            print(f"  [{t.get('likes') or 0}♥ {t['id']}] @{t['author']}: {clean_text(t['text'])[:160]}")


if __name__ == "__main__":
    main()
