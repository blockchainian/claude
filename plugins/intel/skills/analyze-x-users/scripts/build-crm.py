# ABOUTME: Build a local SQLite CRM of the accounts posting about our apps, ranked by post count,
# ABOUTME: with profile (fxtwitter), per-app role/counts, sample posts, and a real/fake verdict.
import json, sqlite3, statistics, os
from collections import defaultdict, Counter

import argparse
parser = argparse.ArgumentParser(description="Build the archive CRM from analyzed account data")
parser.add_argument("--scratch", required=True, help="analysis scratch directory")
parser.add_argument("--apps", required=True, help="comma-separated archive slugs")
args = parser.parse_args()
ROOT = os.getcwd()
SC = args.scratch
APPS = [slug.strip() for slug in args.apps.split(",") if slug.strip()]
if not APPS or any("/" in slug or slug in (".", "..") for slug in APPS):
    parser.error("--apps requires valid archive slugs")
DB = f"{ROOT}/docs/intel/x/crm.sqlite"
TOP_PER_APP = 1000
POSTS_PER_USER_APP = 15

fol = json.load(open(f"{SC}/crm/followers.json"))
def vi(x):
    try: return int(x)
    except: return 0

# per-app: top-500 authors by post count; their role/inbound from authors.json; their posts
per_app_posts = defaultdict(int)   # (handle,app)->count
role = {}                          # (handle,app)->role
inbound = defaultdict(int)         # handle->summed inbound across apps
samples = defaultdict(list)        # (handle,app)->list of post dicts
allviews = defaultdict(list)       # handle->views across apps
alllikes = defaultdict(list)       # handle->likes across apps
apps_of = defaultdict(set)

for app in APPS:
    cnt = Counter()
    for line in open(f"{ROOT}/docs/intel/x/{app}/tweets.jsonl"):
        try: d = json.loads(line)
        except: continue
        a = d.get("author")
        if a: cnt[a] += 1
    top = {a for a, _ in cnt.most_common(TOP_PER_APP)}
    authors = json.load(open(f"{SC}/{app}/authors.json"))
    for a in top:
        role[(a, app)] = (authors.get(a) or {}).get("role", "?")
        inbound[a] += (authors.get(a) or {}).get("inbound", 0)
        per_app_posts[(a, app)] = cnt[a]
        apps_of[a].add(app)
    # labels for sentiment/topic
    lab = {}
    for line in open(f"{ROOT}/docs/intel/x/{app}/labels.jsonl"):
        try: l = json.loads(line); lab[l["id"]] = l
        except: pass
    for line in open(f"{ROOT}/docs/intel/x/{app}/tweets.jsonl"):
        try: d = json.loads(line)
        except: continue
        a = d.get("author")
        if a not in top: continue
        v = vi(d.get("views"))
        allviews[a].append(v); alllikes[a].append(d.get("likes") or 0)
        L = lab.get(d["id"], {})
        samples[(a, app)].append({"id": d["id"], "date": d.get("created_at", ""), "views": v,
            "likes": d.get("likes") or 0, "sentiment": L.get("sentiment"), "topic": L.get("topic"),
            "text": d.get("text", ""), "url": d.get("url", "")})

users = sorted(apps_of.keys())
print("users:", len(users))

# Fake categories are a fixed enum (SQLite has no ENUM type; the users table CHECK-constrains this
# column to exactly these values, '' meaning real). fake_reason keeps the numeric specifics.
FAKE_CATEGORIES = ("demoted", "automated", "low_reach", "suspended")
def verdict(h):
    f = fol.get(h, {})
    if f.get("status") == 404: return 1, "suspended", "account suspended (fxtwitter 404)"
    F, T = f.get("followers"), f.get("tweets")
    n = sum(per_app_posts[(h, ap)] for ap in apps_of[h])
    mv = statistics.median(allviews[h]) if allviews[h] else 0
    al = (sum(alllikes[h]) / len(alllikes[h])) if alllikes[h] else 0
    reach = inbound[h]
    engaged = reach >= max(30, n * 0.2)
    if not engaged and F:
        if F >= 5000 and mv < F * 0.015 and mv < 2000: return 1, "demoted", f"fol {F}, medViews {int(mv)} (<1.5% of followers)"
        if F < 3000 and T and T / F >= 8: return 1, "automated", f"fol {F}, tweets {T} ({round(T/F)}x followers)"
    if n >= 50 and ((mv < 2500 and reach <= max(5, n * 0.01) and al < 8) or (mv < 150 and al < 2)):
        return 1, "low_reach", f"medViews {int(mv)}, avgLikes {round(al,1)}, inbound {reach}"
    return 0, "", ""

os.remove(DB) if os.path.exists(DB) else None
db = sqlite3.connect(DB)
db.executescript("""
CREATE TABLE users(handle TEXT PRIMARY KEY, name TEXT, followers INT, following INT, tweets INT,
  verified INT, bio TEXT, joined TEXT, total_posts INT, apps TEXT, primary_role TEXT,
  inbound INT, median_views INT, is_fake INT,
  fake_category TEXT CHECK(fake_category IN ('demoted','automated','low_reach','suspended','')),
  fake_reason TEXT);
CREATE TABLE user_apps(handle TEXT, app TEXT, posts INT, role TEXT, PRIMARY KEY(handle,app));
CREATE TABLE posts(handle TEXT, app TEXT, tweet_id TEXT, date TEXT, views INT, likes INT,
  sentiment TEXT, topic TEXT, text TEXT, url TEXT);
""")
for h in users:
    f = fol.get(h, {})
    total = sum(per_app_posts[(h, ap)] for ap in apps_of[h])
    main_app = max(apps_of[h], key=lambda ap: per_app_posts[(h, ap)])
    prole = role.get((h, main_app), "?")
    isf, category, reason = verdict(h)
    mv = int(statistics.median(allviews[h])) if allviews[h] else 0
    db.execute("INSERT INTO users VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        (h, f.get("name", ""), f.get("followers"), f.get("following"), f.get("tweets"),
         1 if f.get("verified") else 0, f.get("bio", ""), f.get("joined", ""), total,
         ",".join(sorted(apps_of[h])), prole, inbound[h], mv, isf, category, reason))
    for ap in apps_of[h]:
        db.execute("INSERT INTO user_apps VALUES(?,?,?,?)", (h, ap, per_app_posts[(h, ap)], role.get((h, ap), "?")))
        for p in sorted(samples[(h, ap)], key=lambda x: -(x["views"] or 0))[:POSTS_PER_USER_APP]:
            db.execute("INSERT INTO posts VALUES(?,?,?,?,?,?,?,?,?,?)",
                (h, ap, p["id"], p["date"], p["views"], p["likes"], p["sentiment"], p["topic"], p["text"], p["url"]))
db.commit()
nf = db.execute("SELECT COUNT(*) FROM users WHERE is_fake=1").fetchone()[0]
print(f"users {len(users)}, fake {nf}, posts {db.execute('SELECT COUNT(*) FROM posts').fetchone()[0]}")
db.close()
print("wrote", DB)
