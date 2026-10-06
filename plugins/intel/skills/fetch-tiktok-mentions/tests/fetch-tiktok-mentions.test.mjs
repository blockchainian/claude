// ABOUTME: Tests the pure parts of fetch-tiktok-mentions.mjs: arg and source parsing, the request template,
// ABOUTME: block detection, video merging, source and comment paging, planning, proxies, session draining and the account lookup.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";

import {
  AccountRefused,
  apiUrl,
  Blocked,
  collectComments,
  collectSource,
  downloadVideo,
  drain,
  ispProxyAt,
  loadAccount,
  markAccountExpired,
  mergePulls,
  keepVideos,
  mergeComments,
  mergeVideos,
  openProgress,
  pacer,
  parseArgs,
  parseBody,
  planComments,
  planDownloads,
  proxyDict,
  sharedSessions,
  sourceOf,
  sourcePulls,
  templateFrom,
  usesAccount,
  videosDir,
} from "../scripts/fetch-tiktok-mentions.mjs";

// The shape of a request tiktok.com's own page sends, with made-up ids and signatures.
const PAGE_REQUEST =
  "https://www.tiktok.com/api/explore/item_list/?WebIdLastTime=1790884305&aid=1988&app_name=tiktok_web" +
  "&count=8&device_id=7691789511780943391&odinId=7691789503485576223&region=US&tz_name=America%2FDenver" +
  "&X-Dynosaur=Mx9he2nM&msToken=QffwIdMM&X-Bogus=1&X-Gnarly=M%2FMmGkQl";

const video = (id, createTime, extra = {}) => ({
  id,
  createTime,
  author: { uniqueId: "someone" },
  stats: { commentCount: 5 },
  ...extra,
});

test("args split into the slug, repeatable sources, limits and phase switches", () => {
  const a = parseArgs(["demofun", "--hashtag", "demofun", "--hashtag=exampletag", "--user", "@demodotfun", "--keyword", "Demo  Fun", "--keyword=demo fun"]);
  assert.equal(a.slug, "demofun");
  assert.deepEqual(a.sources, { hashtags: ["demofun", "exampletag"], users: ["demodotfun"], keywords: ["demo fun"] });
  assert.equal(a.commentLimit, 1000);
  assert.equal(a.sourceLimit, 1000);
  assert.equal(a.sessions, null); // every ISP slot
  assert.equal(a.concurrency, 12);
  assert.equal(a.rate, 20);
  assert.deepEqual(a.minPlays, { hashtag: 10000, user: 0, keyword: 0 });
  assert.equal(a.comments, true);
  assert.equal(a.download, true);
  assert.equal(a.dir, "docs/intel/tiktok/demofun");
  assert.equal(parseArgs(["demofun", "--user", "demo", "--out", "/work/raw/tiktok"]).dir, "/work/raw/tiktok");
  assert.equal(parseArgs(["demofun", "--out=elsewhere"]).dir, "elsewhere");

  const b = parseArgs(["x", "--comment-limit", "40", "--source-limit=60", "--sessions", "1", "--no-comments", "--no-download"]);
  assert.deepEqual([b.commentLimit, b.sourceLimit, b.sessions, b.comments, b.download], [40, 60, 1, false, false]);

  const c = parseArgs(["x", "--hashtag-min-plays", "0", "--concurrency", "2", "--rate=5"]);
  assert.equal(c.rate, 5);
  assert.deepEqual(c.minPlays, { hashtag: 0, user: 0, keyword: 0 });
  assert.equal(c.concurrency, 2);

  assert.throws(() => parseArgs(["x", "--bogus"]), /Unknown option/);
  assert.throws(() => parseArgs(["x", "--sessions", "0"]), /positive integer/);
});

test("a source is normalised from a name, a # or @ form, or a tiktok.com url", () => {
  assert.equal(sourceOf("sound", "42"), null); // only hashtags, users and keywords are sources
  assert.deepEqual(sourceOf("keyword", "  Demo   Fun "), { kind: "keyword", value: "demo fun", label: '"demo fun"' });
  assert.equal(sourceOf("keyword", "  "), null);
  assert.deepEqual(sourceOf("hashtag", "#DemoFun"), { kind: "hashtag", value: "demofun", label: "#demofun" });
  assert.deepEqual(sourceOf("hashtag", "https://www.tiktok.com/tag/demofun?lang=en"), {
    kind: "hashtag",
    value: "demofun",
    label: "#demofun",
  });
  assert.deepEqual(sourceOf("user", "https://www.tiktok.com/@Demo.Fun/video/1"), {
    kind: "user",
    value: "demo.fun",
    label: "@demo.fun",
  });
  assert.equal(sourceOf("user", ""), null);
});

test("the request template keeps the page's params and drops its signatures", () => {
  const base = templateFrom(PAGE_REQUEST);
  assert.equal(base.device_id, "7691789511780943391");
  assert.equal(base.tz_name, "America/Denver");
  for (const k of ["X-Bogus", "X-Gnarly", "X-Dynosaur", "msToken"]) assert.equal(k in base, false);
  assert.equal(templateFrom("https://www.tiktok.com/api/share/settings/?aid=1988"), null); // no device_id
  assert.equal(templateFrom("https://www.tiktok.com/explore?device_id=1"), null); // not an api call
});

test("an api url is the template with the endpoint's params laid over it", () => {
  const url = new URL(apiUrl(templateFrom(PAGE_REQUEST), "challenge/item_list/", { challengeID: "9", count: 30, cursor: 60 }));
  assert.equal(url.origin + url.pathname, "https://www.tiktok.com/api/challenge/item_list/");
  assert.equal(url.searchParams.get("count"), "30"); // the endpoint's count replaces the template's
  assert.equal(url.searchParams.get("cursor"), "60");
  assert.equal(url.searchParams.get("device_id"), "7691789511780943391");
  assert.equal(url.searchParams.has("X-Gnarly"), false);
});

test("an empty body blocks the IP, a non-JSON body only the session; JSON is returned parsed", () => {
  assert.deepEqual(parseBody('{"itemList":[]}'), { itemList: [] });
  assert.throws(() => parseBody(""), (e) => e instanceof Blocked && e.scope === "ip");
  assert.throws(() => parseBody(undefined), (e) => e instanceof Blocked && e.scope === "ip");
  assert.throws(() => parseBody("<html>Access Denied</html>"), (e) => e instanceof Blocked && e.scope === "session");
});

test("a first run starts empty; a rerun keeps its state and adds new sources", () => {
  const fresh = openProgress(null, { hashtags: ["a"], users: [], keywords: [] }, 1000);
  assert.deepEqual(fresh, { sources: { hashtags: ["a"], users: [], keywords: [] }, commentLimit: 1000, runs: [], videos: {} });

  const saved = {
    sources: { hashtags: ["a"], users: ["u"] },
    commentLimit: 1000,
    runs: [{ at: "t", fetched: 3, new: 3 }],
    videos: { 1: { comments: { count: 2, complete: true }, downloaded: true } },
  };
  // `saved` predates keywords: it gains them.
  const again = openProgress(saved, { hashtags: ["b", "a"], users: [], keywords: ["k"] }, 1000);
  assert.deepEqual(again.sources, { hashtags: ["a", "b"], users: ["u"], keywords: ["k"] });
  assert.equal(again.runs.length, 1);
  assert.equal(again.videos[1].downloaded, true);

  assert.throws(() => openProgress(saved, { hashtags: [], users: [], keywords: [] }, 500), /commentLimit 1000, not 500/);
});

test("videos merge by id, newest first, recording every source that surfaced them", () => {
  const held = [{ ...video("1", 100), sources: ["#a"] }, { ...video("2", 50), sources: ["#a"] }];
  const { videos, fresh } = mergeVideos(held, [video("2", 50, { desc: "now" }), video("3", 200)], "@u");
  assert.deepEqual(videos.map((v) => v.id), ["3", "1", "2"]);
  assert.equal(fresh, 1);
  assert.deepEqual(videos[2].sources, ["#a", "@u"]);
  assert.equal(videos[2].desc, "now"); // the newer fetch of a video replaces the held one
  assert.deepEqual(videos[0].sources, ["@u"]);
});

test("comments merge by cid in thread order", () => {
  const merged = mergeComments([{ cid: "1" }, { cid: "2" }], [{ cid: "2", text: "t" }, { cid: "3" }]);
  assert.deepEqual(merged.map((c) => c.cid), ["1", "2", "3"]);
  assert.equal(merged[1].text, "t");
});

// A fake TikTok: answers each path from a list of canned pages, recording what was asked.
function fakeApi(pages) {
  const calls = [];
  const call = async (path, params) => {
    calls.push({ path, params });
    const next = pages[path]?.shift();
    if (next instanceof Error) throw next;
    return next ?? {};
  };
  return { call, calls };
}

test("a hashtag is resolved to its id, then paged by cursor to the end", async () => {
  const { call, calls } = fakeApi({
    "challenge/detail/": [{ challengeInfo: { challenge: { id: "77" } } }],
    "challenge/item_list/": [
      { itemList: [video("1", 3), video("2", 2)], hasMore: true, cursor: "30" },
      { itemList: [video("3", 1)], hasMore: false, cursor: "60" },
    ],
  });
  const state = {};
  const result = await collectSource(call, sourceOf("hashtag", "demofun"), 1000, state);
  assert.deepEqual(result, { complete: true, missing: false });
  assert.deepEqual(state.items.map((v) => v.id), ["1", "2", "3"]);
  assert.deepEqual(calls[0], { path: "challenge/detail/", params: { challengeName: "demofun" } });
  assert.deepEqual(calls[1].params, { challengeID: "77", count: 30, cursor: 0 });
  assert.deepEqual(calls[2].params, { challengeID: "77", count: 30, cursor: "30" });
});

test("a user is paged by secUid", async () => {
  const user = fakeApi({
    "user/detail/": [{ userInfo: { user: { secUid: "SEC" } } }],
    "post/item_list/": [{ itemList: [video("1", 1)], hasMore: false, cursor: "-1" }],
  });
  await collectSource(user.call, sourceOf("user", "someone"), 1000, {});
  assert.deepEqual(user.calls[0].params, { uniqueId: "someone", secUid: "" });
  assert.deepEqual(user.calls[1], { path: "post/item_list/", params: { secUid: "SEC", count: 35, cursor: 0 } });
});

test("a keyword is paged by offset, carrying the first page's search id", async () => {
  const { call, calls } = fakeApi({
    "search/item/full/": [
      { status_code: 0, item_list: [video("1", 3), video("2", 2)], has_more: 1, cursor: 20, extra: { logid: "LOG1" } },
      { status_code: 0, item_list: [video("3", 1)], has_more: 0, cursor: 40, extra: { logid: "LOG2" } },
    ],
  });
  const state = {};
  const result = await collectSource(call, sourceOf("keyword", "demo fun"), 1000, state);
  assert.deepEqual(result, { complete: true, missing: false });
  assert.deepEqual(state.items.map((v) => v.id), ["1", "2", "3"]);
  assert.deepEqual(calls[0], { path: "search/item/full/", params: { keyword: "demo fun", offset: 0 } });
  assert.deepEqual(calls[1].params, { keyword: "demo fun", offset: 20, search_id: "LOG1" });
});

test("a search TikTok refuses is an error, not an empty result", async () => {
  const { call } = fakeApi({ "search/item/full/": [{ status_code: 2483, status_msg: "Please log in" }] });
  await assert.rejects(
    collectSource(call, sourceOf("keyword", "demo fun"), 1000, {}),
    (e) => e instanceof AccountRefused && /search refused: 2483 Please log in/.test(e.message),
  );
});

test("a search that only repeats videos has shown all it will", async () => {
  const repeat = { status_code: 0, item_list: [video("1", 3)], has_more: 1, cursor: 20, extra: { logid: "L" } };
  const { call } = fakeApi({ "search/item/full/": [repeat, repeat, repeat, repeat] });
  assert.deepEqual(await collectSource(call, sourceOf("keyword", "a"), 1000, {}), { complete: true, missing: false });
});

// A user whose secUid is already resolved, so a test can start at the list endpoint.
const resolved = (user = "u") => ({ source: sourceOf("user", user), state: { id: "SEC" } });

test("a source TikTok does not know is reported missing without paging", async () => {
  const { call, calls } = fakeApi({ "user/detail/": [{ statusCode: 10221, userInfo: {} }] });
  assert.deepEqual(await collectSource(call, sourceOf("user", "ghost"), 1000, {}), { complete: true, missing: true });
  assert.equal(calls.length, 1);
});

test("paging stops at the source limit, and when TikTok only repeats videos", async () => {
  const capped = fakeApi({
    "post/item_list/": [
      { itemList: [video("1", 3), video("2", 2)], hasMore: true, cursor: "30" },
      { itemList: [video("3", 1)], hasMore: true, cursor: "60" },
    ],
  });
  const { source, state } = resolved();
  assert.deepEqual(await collectSource(capped.call, source, 2, state), { complete: false, missing: false });
  assert.equal(capped.calls.length, 1);

  // A hashtag page that only repeats itself has shown all it will.
  const repeat = { itemList: [video("1", 3)], hasMore: true, cursor: "30" };
  const looping = fakeApi({ "challenge/item_list/": [repeat, repeat, repeat, repeat] });
  const loop = { state: { id: "77" } };
  assert.deepEqual(await collectSource(looping.call, sourceOf("hashtag", "a"), 1000, loop.state), { complete: true, missing: false });
  assert.equal(looping.calls.length, 3); // the first page, then two pages with nothing unseen
  assert.equal(loop.state.items.length, 1);

  // A user's timeline that repeats while TikTok says there is more was cut short: an anonymous
  // viewer gets one page of it.
  const cut = fakeApi({ "post/item_list/": [repeat, repeat, repeat, repeat] });
  const user = resolved();
  assert.deepEqual(await collectSource(cut.call, user.source, 1000, user.state), { complete: false, missing: false });
  assert.equal(user.state.items.length, 1);
});

test("a blocked source keeps its place, so another session continues from the same cursor", async () => {
  const { call, calls } = fakeApi({
    "post/item_list/": [
      { itemList: [video("1", 3)], hasMore: true, cursor: "30" },
      new Blocked("empty"),
      { itemList: [video("2", 2)], hasMore: false },
    ],
  });
  const { source, state } = resolved();
  await assert.rejects(collectSource(call, source, 1000, state), Blocked);
  assert.equal(state.items.length, 1);
  await collectSource(call, source, 1000, state);
  assert.deepEqual(state.items.map((v) => v.id), ["1", "2"]);
  assert.equal(calls[2].params.cursor, "30");
});

test("comments hold each top-level comment followed by its replies, addressed by video id", async () => {
  const { call, calls } = fakeApi({
    "comment/list/": [
      { comments: [{ cid: "a", reply_comment_total: 2 }, { cid: "b", reply_comment_total: 0 }], has_more: 1, cursor: 20 },
      { comments: [{ cid: "c", reply_comment_total: 0 }], has_more: 0, cursor: 40 },
    ],
    "comment/list/reply/": [
      { comments: [{ cid: "a1" }], has_more: 1, cursor: 1 },
      { comments: [{ cid: "a2" }], has_more: 0, cursor: 2 },
    ],
  });
  const state = {};
  assert.deepEqual(await collectComments(call, "V", 1000, state), { complete: true });
  assert.deepEqual(state.comments.map((c) => c.cid), ["a", "a1", "a2", "b", "c"]);
  // Every top-level page first, then the replies.
  assert.deepEqual(calls[0].params, { aweme_id: "V", count: 20, cursor: 0 });
  assert.equal(calls[1].params.cursor, 20);
  assert.deepEqual(calls[2], { path: "comment/list/reply/", params: { item_id: "V", comment_id: "a", count: 20, cursor: 0 } });
  assert.equal(calls[3].params.cursor, 1);
});

test("the most liked and replied comments come first, and the limit's room for replies goes to them a page each in turn", async () => {
  const { call, calls } = fakeApi({
    "comment/list/": [
      {
        comments: [
          { cid: "x", digg_count: 1, reply_comment_total: 1 },
          { cid: "y", digg_count: 50, reply_comment_total: 40 },
          { cid: "z", digg_count: 5, reply_comment_total: 0 },
        ],
        has_more: 0,
      },
    ],
    "comment/list/reply/": [
      { comments: [{ cid: "y1" }], has_more: 1, cursor: 1 },
      { comments: [{ cid: "x1" }], has_more: 0 },
      { comments: [{ cid: "y2" }, { cid: "y3" }], has_more: 1, cursor: 3 },
    ],
  });
  const state = {};
  assert.deepEqual(await collectComments(call, "V", 6, state), { complete: true });
  // 3 top-level leave room for 3 replies: y's first page, x's only page, then y again, cut at the limit.
  assert.deepEqual(state.comments.map((c) => c.cid), ["y", "y1", "y2", "z", "x", "x1"]);
  assert.deepEqual(calls.slice(1).map((c) => c.params.comment_id), ["y", "x", "y"]);
});

test("top-level comments beyond the limit are not paged", async () => {
  const { call, calls } = fakeApi({
    "comment/list/": [{ comments: [{ cid: "a", reply_comment_total: 5 }, { cid: "b" }], has_more: 1, cursor: 20 }],
  });
  const state = {};
  assert.deepEqual(await collectComments(call, "V", 2, state), { complete: true });
  assert.deepEqual(state.comments.map((c) => c.cid), ["a", "b"]);
  assert.equal(calls.length, 1); // no second page, no replies: the limit is full
});

test("a video with comments turned off ends at once with none", async () => {
  const { call } = fakeApi({ "comment/list/": [{ comments: null, has_more: 0 }] });
  const state = {};
  assert.deepEqual(await collectComments(call, "V", 1000, state), { complete: true });
  assert.deepEqual(state.comments, []);
});

test("blocked comments resume without refetching top-level pages or finished reply threads", async () => {
  const { call, calls } = fakeApi({
    "comment/list/": [{ comments: [{ cid: "a", reply_comment_total: 1 }, { cid: "b", reply_comment_total: 1 }], has_more: 0 }],
    "comment/list/reply/": [{ comments: [{ cid: "a1" }], has_more: 0 }, new Blocked("empty"), { comments: [{ cid: "b1" }], has_more: 0 }],
  });
  const state = {};
  await assert.rejects(collectComments(call, "V", 1000, state), Blocked);
  assert.deepEqual(state.comments.map((c) => c.cid), ["a", "a1", "b"]); // what was fetched so far is kept
  await collectComments(call, "V", 1000, state);
  assert.deepEqual(state.comments.map((c) => c.cid), ["a", "a1", "b", "b1"]);
  assert.equal(calls.filter((c) => c.path === "comment/list/").length, 1);
  assert.equal(calls.filter((c) => c.params.comment_id === "a").length, 1);
});

test("comments are planned for videos not yet complete; downloads for videos without a file", () => {
  const videos = [video("1", 3), video("2", 2), video("3", 1, { imagePost: { images: [] } }), video("4", 0)];
  const progress = {
    videos: {
      1: { comments: { count: 4, complete: true }, downloaded: true },
      2: { comments: { count: 1, complete: false }, downloaded: false },
      4: { comments: { count: 0, complete: true }, downloaded: false },
    },
  };
  assert.deepEqual(planComments(videos, progress).map((u) => u.id), ["2", "3"]);
  // The file on disk decides, not the progress file: the files live outside the repo and can be gone.
  const noFiles = () => false;
  // 3 is a photo post, no file to fetch.
  assert.deepEqual(planDownloads(videos, progress, noFiles).map((u) => u.id), ["1", "2", "4"]);
  assert.deepEqual(planDownloads(videos, progress, (id) => id === "1").map((u) => u.id), ["2", "4"]);
});

// A fake session for downloads: `call` answers item/detail, `fetchBytes` answers the play address.
test("a download resolves a fresh play address in its session, then fetches the bytes there", async () => {
  const written = [];
  const fetched = [];
  const deps = {
    call: async (path, params) => {
      assert.equal(path, "item/detail/");
      return params.itemId === "gone"
        ? { statusCode: 10204 }
        : { statusCode: 0, itemInfo: { itemStruct: { video: { playAddr: `https://v16/${params.itemId}?sig` } } } };
    },
    fetchBytes: async (url) => {
      fetched.push(url);
      return url.includes("expired") ? { status: 403, bytes: null } : { status: 200, bytes: Buffer.from("mp4") };
    },
    write: async (path, bytes) => written.push([path, bytes.toString()]),
  };
  assert.equal(await downloadVideo({ id: "7" }, "/vids", deps), "downloaded");
  assert.deepEqual(written, [["/vids/7.mp4", "mp4"]]);
  assert.deepEqual(fetched, ["https://v16/7?sig"]);

  assert.equal(await downloadVideo({ id: "gone" }, "/vids", deps), "gone"); // TikTok no longer has it
  assert.equal(fetched.length, 1); // nothing fetched for it
  assert.equal(await downloadVideo({ id: "expired" }, "/vids", deps), "failed 403");
  assert.equal(written.length, 1);
});

test("a proxy url becomes the browser's proxy settings; an ISP slot is the base port plus the slot", () => {
  assert.deepEqual(proxyDict("http://customer-a%40b:p%3Aw@pr.example.io:7777"), {
    server: "http://pr.example.io:7777",
    username: "customer-a@b",
    password: "p:w",
  });
  assert.equal(ispProxyAt("http://u:p@isp.example.io:8000", 1), "http://u:p@isp.example.io:8001");
  assert.equal(ispProxyAt("http://u:p@isp.example.io:8000", 10), "http://u:p@isp.example.io:8010");
});

test("a blocked session is replaced, with its error, and its unit goes back on the queue", async () => {
  const opened = [];
  const closed = [];
  const done = [];
  let blockOnce = true;
  const { left } = await drain({
    lanes: 1,
    units: ["a", "b", "c"],
    open: async (lane) => {
      const session = `s${opened.length}`;
      opened.push(lane);
      return session;
    },
    close: async (session, failed) => closed.push(`${session}:${failed ? failed.scope : "done"}`),
    work: async (_session, unit) => {
      if (unit === "b" && blockOnce) {
        blockOnce = false;
        throw new Blocked("Access Denied", "session");
      }
      done.push(unit);
    },
    log: () => {},
  });
  assert.equal(left, 0);
  assert.deepEqual(done.sort(), ["a", "b", "c"]);
  assert.deepEqual(opened, [0, 0]); // the lane's first session, then its replacement
  assert.deepEqual(closed, ["s0:session", "s1:done"]); // every session opened is closed
});

test("a lane stops after repeated failures and reports the units left", async () => {
  const lines = [];
  const { left } = await drain({
    lanes: 1,
    units: ["a", "b"],
    open: async () => "s",
    close: async () => {},
    work: async () => {
      throw new Blocked("empty");
    },
    log: (line) => lines.push(line),
  });
  assert.equal(left, 2);
  assert.match(lines.at(-1), /stopped/);
});

test("lanes share one queue, each through its own session", async () => {
  const used = new Set();
  const done = [];
  const { left } = await drain({
    lanes: 2,
    units: ["a", "b", "c", "d"],
    open: async (lane) => `s${lane}`,
    close: async () => {},
    work: async (session, unit) => {
      used.add(session);
      await new Promise((r) => setTimeout(r, 5));
      done.push(unit);
    },
    log: () => {},
  });
  assert.equal(left, 0);
  assert.deepEqual(done.sort(), ["a", "b", "c", "d"]);
  assert.deepEqual([...used].sort(), ["s0", "s1"]);
});

test("video files live outside the repo, in ~/.local/share/tiktok unless overridden", () => {
  assert.equal(videosDir({}), join(homedir(), ".local", "share", "tiktok"));
  assert.equal(videosDir({ INTEL_TIKTOK_VIDEO_DIR: "/data/tt" }), "/data/tt");
  assert.equal(videosDir({ INTEL_TIKTOK_VIDEO_DIR: "~/tt" }), join(homedir(), "tt"));
});

test("a source keeps only the videos at or above its play floor", () => {
  const items = [video("1", 1, { stats: { playCount: 9999 } }), video("2", 1, { stats: { playCount: 10000 } }), video("3", 1, {})];
  const minPlays = { hashtag: 10000, user: 0, keyword: 0 };
  assert.deepEqual(keepVideos(sourceOf("hashtag", "a"), items, minPlays).map((v) => v.id), ["2"]);
  assert.deepEqual(keepVideos(sourceOf("keyword", "k"), items, minPlays).map((v) => v.id), ["1", "2", "3"]);
  assert.deepEqual(keepVideos(sourceOf("user", "u"), items, minPlays).map((v) => v.id), ["1", "2", "3"]);
});

test("a source keeps only English videos; a caption of hashtags alone has no language and stays", () => {
  const items = [
    video("1", 1, { textLanguage: "en" }),
    video("2", 1, { textLanguage: "es" }),
    video("3", 1, { textLanguage: "un" }), // TikTok could not tell: only hashtags
    video("4", 1, {}),
  ];
  assert.deepEqual(keepVideos(sourceOf("user", "u"), items, { hashtag: 0, user: 0 }).map((v) => v.id), ["1", "3", "4"]);
});

test("a hashtag is pulled once per session, a user and a keyword once; a source's pulls merge into one list", () => {
  const tag = sourceOf("hashtag", "demofun");
  const user = sourceOf("user", "someone");
  const pulls = sourcePulls([tag, user, sourceOf("keyword", "demo fun")], 3);
  assert.deepEqual(pulls.map((p) => p.source.label), ["#demofun", "#demofun", "#demofun", "@someone", '"demo fun"']);

  // Each pull of a hashtag page is a different sample of it.
  pulls[0].state.items = [video("1", 3), video("2", 2)];
  pulls[0].result = { complete: true, missing: false };
  pulls[1].state.items = [video("2", 2), video("3", 1)];
  pulls[1].result = { complete: true, missing: false };
  // the third pull never finished
  pulls[3].result = { complete: true, missing: true };
  const [tagged, mentioned] = mergePulls(pulls);
  assert.equal(tagged.source, tag);
  assert.deepEqual(tagged.items.map((v) => v.id), ["1", "2", "3"]);
  assert.deepEqual([tagged.pulls, tagged.complete, tagged.missing], [3, false, false]);
  assert.deepEqual([mentioned.items, mentioned.pulls, mentioned.complete, mentioned.missing], [[], 1, true, true]);
});

test("lanes share one session per slot; a failed session is replaced once, after a cool-down", async () => {
  const opened = [];
  const closed = [];
  const waits = [];
  const pool = sharedSessions({
    count: 2,
    openOne: async (slot) => {
      const session = { slot, n: opened.length };
      opened.push(slot);
      return session;
    },
    closeOne: async (session) => closed.push(session.n),
    cooldownMs: 60000,
    wait: async (ms) => waits.push(ms),
  });
  const [a, b, c, d] = await Promise.all([0, 1, 2, 3].map((lane) => pool.open(lane)));
  assert.deepEqual(opened.sort(), [1, 2]); // four lanes, two sessions, slots are 1-based
  assert.equal(a, c);
  assert.equal(b, d);
  assert.notEqual(a, b);

  // Two lanes report the same session blocked at the IP level: it is closed once (the second report
  // is told so), and reopened once after the cool-down.
  const ipBlock = new Blocked("empty response", "ip");
  assert.deepEqual(await Promise.all([pool.close(a, ipBlock), pool.close(c, ipBlock)]), [true, false]);
  assert.deepEqual(closed, [a.n]);
  const [a2, c2] = await Promise.all([pool.open(0), pool.open(2)]);
  assert.equal(a2, c2);
  assert.notEqual(a2, a);
  assert.deepEqual(waits, [60000]);

  // A session refused on its own account (the IP is fine) is replaced at once, no cool-down.
  await pool.close(a2, new Blocked("non-JSON response", "session"));
  assert.deepEqual(closed, [a.n, a2.n]);
  const a3 = await pool.open(0);
  assert.notEqual(a3, a2);
  assert.deepEqual(waits, [60000]);

  // At the end every lane lets go; a session closes when its last lane does, without a cool-down.
  await pool.close(b, null);
  assert.deepEqual(closed, [a.n, a2.n]);
  await pool.close(d, null);
  assert.deepEqual(closed, [a.n, a2.n, b.n]);
});

test("a session that fails to open is retried by the next lane", async () => {
  let tries = 0;
  const pool = sharedSessions({
    count: 1,
    openOne: async () => {
      if (tries++ === 0) throw new Error("no template");
      return { ok: true };
    },
    closeOne: async () => {},
    cooldownMs: 0,
    wait: async () => {},
  });
  await assert.rejects(pool.open(0), /no template/);
  assert.deepEqual(await pool.open(0), { ok: true });
});

test("a pacer spaces request starts evenly at the rate, whoever asks", async () => {
  let now = 1000;
  const waits = [];
  const next = pacer(20, () => now, async (ms) => { waits.push(ms); now += ms; });
  await next(); // the first goes at once
  await next(); // 50 ms later
  now += 10;
  await next(); // 40 ms more, so starts stay 50 ms apart
  now += 500; // a long pause does not bank credit
  await next();
  await next();
  assert.deepEqual(waits, [0, 50, 40, 0, 50]);
});

test("a lane whose shared session another lane already replaced is not charged a failure", async () => {
  const lines = [];
  let calls = 0;
  const done = [];
  const { left } = await drain({
    lanes: 1,
    units: ["a", "b", "c", "d"],
    open: async () => "s",
    close: async () => false, // the pool says: this session was already handed back by another lane
    work: async (_session, unit) => {
      if (calls++ < 3) throw new Error("Target page, context or browser has been closed");
      done.push(unit);
    },
    log: (line) => lines.push(line),
  });
  assert.equal(left, 0);
  assert.equal(done.length, 4);
  assert.equal(lines.some((l) => /stopped/.test(l)), false);
});

// A secrets-manager store (its own schema) holding the given tiktok rows.
function storeWith(rows) {
  const dir = mkdtempSync(join(tmpdir(), "tiktok-store-"));
  const db = new DatabaseSync(join(dir, "secrets.sqlite"));
  db.exec(`CREATE TABLE tiktok (username TEXT PRIMARY KEY, password TEXT, cookies TEXT, isp_slot INTEGER, status TEXT, created_at TEXT, updated_at TEXT)`);
  for (const [username, slot, status] of rows) {
    db.prepare(
      "INSERT INTO tiktok (username, password, cookies, isp_slot, status, created_at, updated_at) VALUES (?, 'pw', '[]', ?, ?, 't', 't')",
    ).run(username, slot, status);
  }
  db.close();
  return { INTEL_SECRETS_STATE_DIR: dir };
}

test("the account is the store's first active TikTok login, with its profile and ISP slot", () => {
  const env = storeWith([["zed", 3, "active"], ["bob", 10, "active"], ["al", 2, "expired"]]);
  assert.deepEqual(loadAccount(env), {
    username: "bob",
    slot: 10,
    profile: join(env.INTEL_SECRETS_STATE_DIR, "profiles", "bob"),
  });
  assert.equal(loadAccount(storeWith([["al", 2, "expired"]])), null);
  assert.equal(loadAccount({ INTEL_SECRETS_STATE_DIR: mkdtempSync(join(tmpdir(), "tiktok-nostore-")) }), null);
});

test("an account whose session is gone is marked expired, so the next run does not pick it", () => {
  const env = storeWith([["bob", 10, "active"]]);
  markAccountExpired("bob", env);
  assert.equal(loadAccount(env), null);
});

test("keywords always go through the account; users when there is one; hashtags never", () => {
  const [tag, user, keyword] = [sourceOf("hashtag", "a"), sourceOf("user", "u"), sourceOf("keyword", "k")];
  assert.deepEqual([tag, user, keyword].map((s) => usesAccount(s, true)), [false, true, true]);
  assert.deepEqual([tag, user, keyword].map((s) => usesAccount(s, false)), [false, false, true]);
});
