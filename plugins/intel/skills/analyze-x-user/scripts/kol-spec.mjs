// ABOUTME: The labeling spec for one X account's own timeline: rules for reading a KOL's posts, the fields
// ABOUTME: (what kind of post, about what, which asset, which way, with what stake), the answer schema, the vocab lists.

export const FIELDS = ["id", "about", "kind", "topic", "asset", "stance", "point", "interest"];
// The model returns a running number per post, never the 19-digit id: small models mistype long ids.
const OUT_FIELDS = ["n", ...FIELDS.slice(1)];

export const KINDS = ["call", "analysis", "pnl", "news", "promo", "banter", "noise"];
export const STANCES = ["bullish", "bearish", "neutral", "none"];

export const SCHEMA = {
  type: "object",
  additionalProperties: false,
  required: ["labels"],
  properties: {
    labels: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: OUT_FIELDS,
        properties: {
          n: { type: "integer" },
          about: { type: "boolean" },
          kind: { type: "string", enum: KINDS },
          topic: { type: "string" },
          asset: { type: "string" },
          stance: { type: "string", enum: STANCES },
          point: { type: "string" },
          interest: { type: ["string", "null"] },
        },
      },
    },
  },
};

export const RULES = `
You label the posts of ONE X account for a profile of that account: what it posts, which assets it
pushes and which way, and what stake it has. There is no app under review. Everything you need is in
the message: do not run commands, do not read or write files. Reply with the JSON only:
{"labels": [...]}, one object per post, in the order given, every post number present exactly once,
fields {n, about, kind, topic, asset, stance, point, interest}.
Posts are one per line: n, author, likes, date, lang, text (tab separated). Every post is by the
account itself; most are replies inside other people's threads, so read a reply as the account's own
words, not as a comment on an app.
about is true when the post carries content of its own: a view, a call, a trade, a number, a story, a
recommendation, a joke with a target, an answer with substance. It is false when the post says nothing
on its own: gm, emoji, "哈哈", one-word agreement, a bare tag, a bare link, a giveaway entry.
Read the stance from what the account means, not the wording: mocking a coin is bearish, "上车了" is
bullish, a warning is bearish, a shill is bullish. A stance is only about the named asset; a market-wide
view goes on the market (asset "market") with the stance it shows. When the post names no asset and
takes no market view, asset is "none" and stance is "none".
The stake (interest) is what we most want to know about this account: whether it holds, issues,
advertises or gets paid for what it pushes. Take it only from what the post itself shows (a referral
link or code, "广告", "合作", "我们的币", "我发的", "进群", an affiliate program, an airdrop task); never guess.
All text fields are English whatever the post's language; asset keeps the ticker or project name.
`;

// The fields, with the vocabulary seen so far across accounts as examples (never a closed list).
export function fields(vocab = {}) {
  const list = (k) => (vocab[k] || []).join(", ") || "none yet";
  return `FIELDS (one object per post)
- about: true when the post has content of its own (see the rules), false when it is content-free.
- kind: call (a buy / sell / entry / exit / target on a named asset) · analysis (a thesis, market view, thread, explainer, lesson) · pnl (a trade result, a position, a portfolio, a win or loss shown) · news (reporting or reacting to an event: a listing, a hack, a launch, a person, a price move) · promo (pushing a project, product, group, giveaway, referral or service) · banter (social chatter, life, jokes, replies with a view but no asset or event) · noise (about=false).
- topic: what the post is about as a subject, a 2-4 word English noun phrase, lowercase with hyphens, specific enough to tell apart and reused for every post about the same thing. Examples: ${list("topics")}. If none fits, write your own; never an "other" bucket; "none" when about=false.
- asset: the ticker or project the post is about (BTC, BNB, ETH, SOL, a meme ticker, an exchange or protocol name in lowercase), "market" for a market-wide view, "none" when no asset or market is meant.
- stance: bullish / bearish / neutral toward the asset (or the market), "none" when asset is "none".
- point: at most 12 words saying what the post claims (the call, the number, the take), never the asset name alone. Empty string when there is no claim.
- interest: null when the post shows no stake; otherwise the kind of stake. Examples: ${list("interests")} (own-token: promotes a token it issued or holds a team role in; referral: posts a code or link; sponsored: says it is an ad, a partnership or paid; exchange-affiliate: an exchange's KOL or agent program; paid-group: sells a paid community, course or signal group; creator-rewards: earns platform callout / thesis rewards; airdrop-farming: does a task for an airdrop; team-member: works for the project named). Write your own if none fits.
`;
}

// Which label field feeds which vocab list when merge-labels grows the vocabulary; assets are per
// account and stay out of the shared vocab.
export const VOCAB = { topic: "topics", interest: "interests" };

export const isNoise = (l) => l.kind === "noise";
