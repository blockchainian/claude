#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.13"
# dependencies = ["numpy"]
# ///
# ABOUTME: Tests topics.py on a tiny fixture with injected encoder, clusterer and sentiment callables:
# ABOUTME: incremental caching by id, window filtering, the topic table, timeline and label export.
import json
import sys
import tempfile
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import topics as T  # noqa: E402


def tweet(n, author, text, day, likes=0, lang="en"):
    return {"id": "2000000000000000000" + str(n), "author": author, "text": text, "likes": likes,
            "replies": 0, "lang": lang, "created_at": f"Mon Sep {day} 10:00:00 +0000 2026"}


FIX = [
    tweet(1, "alice", "fees are 0% now, nice", 2, 50),
    tweet(2, "bob", "fees went up again, terrible", 3, 9),
    tweet(3, "carol", "no airdrop, they promised it a year ago", 4, 3),
    tweet(4, "dave", "hola amigos", 4, 1, lang="es"),
    tweet(5, "erin", "gm gm gm", 5, 2),
]

ENCODED = []


def encode(texts):
    ENCODED.extend(texts)
    # "fee" texts → +x, others → -x, so the fake clusterer separates them
    # "fee" → +x, "airdrop" → -x, anything else → an outlier that sits nearest cluster 1
    return np.array([[1.0, 0.0] if "fee" in t else [-1.0, 0.0] if "airdrop" in t else [0.0, 1.0] for t in texts], dtype=np.float32)


def sentiment(texts):
    return ["like" if "nice" in t else "dislike" if ("terrible" in t or "no " in t) else "neutral" for t in texts]


class FakeClusterer:
    keywords = {0: ["fees", "fee"], 1: ["airdrop"]}

    def fit(self, docs, emb):
        return self.transform(docs, emb)

    def transform(self, docs, emb):
        # x>0 → cluster 0, x<0 → cluster 1, x==0 → outlier
        return np.array([0 if e[0] > 0 else 1 if e[0] < 0 else -1 for e in emb], dtype=np.int32)

    def nearest(self, emb):
        return np.array([1 if e[1] > 0 else 0 for e in emb], dtype=np.int32)


def test_cache_encodes_only_new_english_ids():
    with tempfile.TemporaryDirectory() as d:
        cache = T.Cache(Path(d))
        ENCODED.clear()
        new = T.update_cache(FIX[:2], cache, encode, sentiment)
        assert new == 2 and len(ENCODED) == 2
        cache.save()
        cache = T.Cache(Path(d))
        new = T.update_cache(FIX, cache, encode, sentiment)
        assert new == 2, "the two english tweets not yet cached; spanish is never encoded"
        assert ENCODED[-2].startswith("no airdrop")
        assert cache.ids == [t["id"] for t in FIX if t["lang"] == "en"]
        assert cache.sentiment == ["like", "dislike", "dislike", "neutral"]
        assert cache.emb.dtype == np.float16


def test_refit_assigns_every_cached_tweet_and_hot_run_assigns_only_new():
    with tempfile.TemporaryDirectory() as d:
        cache = T.Cache(Path(d))
        T.update_cache(FIX[:2], cache, encode, sentiment)
        cl = FakeClusterer()
        T.refit(cache, cl, sample=10)
        assert cache.topic.tolist() == [0, 0]
        assert cache.keywords == {"0": ["fees", "fee"], "1": ["airdrop"]}
        T.update_cache(FIX, cache, encode, sentiment)
        assert cache.topic.tolist() == [0, 0, T.UNASSIGNED, T.UNASSIGNED]
        T.assign_new(cache, cl)
        assert cache.topic.tolist() == [0, 0, 1, 1], "the outlier lands in its nearest cluster"
        assert cache.outlier.tolist() == [False, False, False, True]


def named_cache(d):
    cache = T.Cache(Path(d))
    T.update_cache(FIX, cache, encode, sentiment)
    T.refit(cache, FakeClusterer(), sample=10)
    cache.names = {"0": "手续费"}
    return cache


def test_table_uses_names_then_keywords_and_buckets_other_languages():
    with tempfile.TemporaryDirectory() as d:
        rows = T.table(FIX, named_cache(d))
        assert [list(r.keys()) for r in rows][0] == ["topic", "n", "authors", "likes", "peakDay", "peakN", "keywords"]
        assert [(r["topic"], r["n"], r["authors"], r["likes"], r["peakDay"], r["peakN"]) for r in rows] == [
            ("手续费", 2, 2, 59, "2026-09-02", 1),
            ("airdrop", 2, 2, 5, "2026-09-04", 1),
            (T.OTHER_LANG, 1, 1, 1, "2026-09-04", 1),
        ]
        assert rows[0]["keywords"] == "fees, fee"


def test_clusters_sharing_a_name_are_one_row_and_noise_is_not_about():
    with tempfile.TemporaryDirectory() as d:
        cache = named_cache(d)
        cache.names = {"0": "手续费", "1": "手续费"}
        rows = T.table(FIX, cache)
        assert [(r["topic"], r["n"], r["keywords"]) for r in rows] == [("手续费", 4, "fees, fee, airdrop"), (T.OTHER_LANG, 1, "")]
        cache.names = {"0": T.NOISE}
        assert [l["about"] for l in T.labels(FIX, cache)] == [False, False, True, False, True]


def test_window_filters_at_group_by_only():
    with tempfile.TemporaryDirectory() as d:
        cache = named_cache(d)
        rows = T.table(FIX, cache, since="2026-09-03", until="2026-09-04")
        assert [(r["topic"], r["n"]) for r in rows] == [("手续费", 1)]
        days = T.timeline(FIX, per_day=1, since="2026-09-04", until="2026-09-05")
        assert [(x["day"], x["n"], x["top"][0]["id"]) for x in days] == [("2026-09-04", 2, FIX[2]["id"])]
        assert [l["id"] for l in T.labels(FIX, cache, since="2026-09-04", until="2026-09-05")] == [FIX[2]["id"], FIX[3]["id"]]
        assert len(cache.ids) == 4, "window never shrinks the cache"


def test_labels_and_clusters_for_naming():
    with tempfile.TemporaryDirectory() as d:
        cache = named_cache(d)
        labels = T.labels(FIX, cache)
        assert labels == [
            {"id": FIX[0]["id"], "about": True, "sentiment": "like", "topic": "手续费"},
            {"id": FIX[1]["id"], "about": True, "sentiment": "dislike", "topic": "手续费"},
            {"id": FIX[2]["id"], "about": True, "sentiment": "dislike", "topic": "airdrop"},
            {"id": FIX[3]["id"], "about": False, "sentiment": "neutral", "topic": T.OTHER_LANG},
            {"id": FIX[4]["id"], "about": True, "sentiment": "neutral", "topic": "airdrop"},
        ]
        unnamed = T.clusters_for_naming(FIX, cache, k=10)
        assert list(unnamed) == ["1"]
        assert unnamed["1"]["keywords"] == ["airdrop"]
        assert unnamed["1"]["examples"] == [{"id": FIX[2]["id"], "likes": 3, "text": FIX[2]["text"]},
                                            {"id": FIX[4]["id"], "likes": 2, "text": FIX[4]["text"]}]


def test_names_file_round_trip_and_inheritance_across_refit():
    with tempfile.TemporaryDirectory() as d:
        cache = named_cache(d)
        path = Path(d) / "names.json"
        T.write_names(path, cache)
        assert json.loads(path.read_text()) == {"0": {"name": "手续费", "keywords": ["fees", "fee"]}}
        assert T.read_names(path) == {"0": "手续费"}
        old = json.loads(path.read_text())
        assert T.inherit_names(old, {"7": ["fee", "fees", "cost"], "8": ["airdrop"]}) == {"7": "手续费"}
        assert T.inherit_names(old, {"7": ["fee", "a", "b", "c"]}) == {}, "1 of 5 shared is below the bar"


def test_day_of_handles_twitter_and_iso_dates():
    assert T.day_of({"created_at": "Tue Sep 22 03:59:19 +0000 2026"}) == "2026-09-22"
    assert T.day_of({"created_at": "2026-09-21T23:59:59+00:00"}) == "2026-09-21"
    assert T.day_of({"created_at": "Mon Sep 2 10:00:00 +0000 2026"}) == "2026-09-02"


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for t in tests:
        t()
        print("ok", t.__name__)
    print(f"{len(tests)} passed")
