// ABOUTME: The labeling spec for an app's X mentions: the rules the model runs under, the eight fields
// ABOUTME: with the shared vocabulary as examples, the JSON schema of its answer and which vocab lists grow.

export const FIELDS = ["id", "about", "sentiment", "topic", "feature", "point", "request", "interest"];
// The model returns a running number per post, never the 19-digit id: small models mistype long ids.
const OUT_FIELDS = ["n", ...FIELDS.slice(1)];

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
          sentiment: { type: "string", enum: ["like", "dislike", "neutral", "noise", "irrelevant"] },
          topic: { type: "string" },
          feature: { type: "string" },
          point: { type: "string" },
          request: { type: ["string", "null"] },
          interest: { type: ["string", "null"] },
        },
      },
    },
  },
};

export const RULES = `
You label X posts for a reception report. Everything you need is in the message: do not run commands,
do not read or write files. Reply with the JSON only: {"labels": [...]}, one object per post, in the
order given, every post number present exactly once, fields
{n, about, sentiment, topic, feature, point, request, interest}.
Posts are one per line: n, author, likes, date, lang, text (tab separated).
Most posts are replies inside threads. A reply is about the app whenever what it says concerns the app:
its features, fees, slippage, alerts, verification, listings, outages, scams on it, its team, or a promise
it made; that holds when the reply is short, sarcastic, a question, or a rally cry. It is irrelevant only
when it talks about something else (another token, a person, a different product). It is noise only when
it says nothing (gm, emoji, "wen", address drops, giveaway begging, bot alerts, bare referral-code spam).
Sentiment is the author's attitude toward the app as the post shows it: positive is like, negative is
dislike, no attitude is neutral. Judge the attitude, not the wording or the form: a win or milestone
credited to the app, a recommendation, thanks, hype, or joy at using it are positive whatever the phrasing;
a complaint, a doubt, a demand, sarcasm, or disappointment are negative whatever the phrasing. The attitude
must be toward the app itself, not toward a token, a trade or a trader: a post that is bullish on a token,
shares a position, a thesis or an entry, or hands out a referral code, and says nothing about the app, is
neutral. A referral code next to an opinion about the app does not cancel the opinion; the stake goes in interest.
All text fields are English whatever the post's language.
`;

// The eight fields, with the vocabulary seen so far as examples (never a closed list).
export function fields(vocab = {}) {
  const list = (k) => (vocab[k] || []).join(", ") || "none yet";
  return `FIELDS (one object per post)
- about: true only if the post is about the app itself.
- sentiment: for about=true, like / dislike / neutral / noise (noise: about the app but content-free); for about=false, irrelevant (talks about something else) or noise (content-free).
- topic: what the post is about as a subject people discuss (the event, the company, the ecosystem, the culture). Examples: ${list("topics")}. If none fits, write your own. Never an "other" bucket. "none" when about=false.
- feature: which part of the app the post is about. Examples: ${list("features")}. If none fits, write your own; "none" when no part of the app applies (using the app is not a feature).
- A topic or feature you write yourself must be a 2-4 word English noun phrase, lowercase with hyphens, specific enough to tell apart from the examples, and reused for every post about the same thing.
- point: at most 12 words saying what the post claims (the bug, the number, the complaint), never the feature name alone. Empty string when there is no claim.
- request: at most 12 words when the post asks to add, fix, change or remove something, else null.
- interest: null when the speaker has no stake; otherwise the kind of stake. Examples: ${list("interests")} (referral: posts a code or link; creator-rewards: earns callout / thesis rewards; token-team: promotes their own token; official-partner: the company, staff, partners; paid-promotion). Write your own if none fits.
`;
}

// Which label field feeds which vocab list when merge-labels grows the vocabulary.
export const VOCAB = { topic: "topics", feature: "features", interest: "interests" };

// A content-free post, for the per-chunk summary line.
export const isNoise = (l) => l.sentiment === "noise";
