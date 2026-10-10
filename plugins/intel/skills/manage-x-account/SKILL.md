---
name: manage-x-account
description: >-
  Preview and perform X account actions through the user's logged-in opencli account: reply, quote, post, like, unlike, follow or unfollow, individually or from a plan file. Use when asked to act on X. NOT for reading the feed (use fetch-x-feed), NOT for searches or threads (use fetch-x-posts), and NOT for account archives (use fetch-x-user-posts).
---

# manage-x-account

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
script imports from sibling skills, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin's `.env.example` to the shared intel `.env` configuration file. The script
loads it without replacing variables already exported in the shell. Resolve the configuration
location and storage roots through `fetch-x-mentions/scripts/env.mjs`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; attempted writes are logged to `x/writes.jsonl` | No | Shared intel `.env` |

## Setup

Use Node.js 22.13+ and have `opencli` installed on `PATH`. Ask the user to run
`opencli twitter login` if needed, then check `opencli twitter whoami -f json` to confirm the
account. Do not attempt a login yourself.

## Preview and act

Compose an action or a JSON plan, then run the preview:

```sh
node "$SKILL_DIR/scripts/manage-x-account.mjs" reply "<status-url>" "<text>"
node "$SKILL_DIR/scripts/manage-x-account.mjs" --plan actions.json
```

Show the user the preview before passing `--send`. Reviewing is not approving: do not send on
the same turn you compose unless the user asked for exactly that text. Use the same reviewed
text and targets when sending:

```sh
node "$SKILL_DIR/scripts/manage-x-account.mjs" --plan actions.json --send
```

A dry run sends nothing and exits successfully even if the account could not be confirmed.
Inspect the preview's account, target and full text; `--send` requires account confirmation.

## Command reference

| Verb | Positional arguments |
| --- | --- |
| `reply`, `quote` | `<status-url> <text>` |
| `post` | `<text>` |
| `like`, `unlike` | `<status-url>` |
| `follow`, `unfollow` | `<user>` |

Use `--plan <file>` instead of positional arguments for a JSON list of action objects. Each
object has `verb` and the corresponding `url`, `text` or `user` fields. `--send` is optional and
is the only way to send. Status targets are post URLs; user targets may have a leading `@`.

## Failures and limits

One invalid action refuses the entire batch. A failed write may have landed: failures are never
retried, and you must check X before running that action again. Every attempted write, successful
or failed, appends `at`, `account`, `action`, `ok` and `result` to the JSONL log.

Follows are paced at least 60 seconds apart and capped at 15 attempts per run; excess follows
are skipped, not queued. The observed allowance was roughly 15 per 15 minutes; error 161 is the
daily cap. Long batches run in the background with the Bash tool's timeout field, never shell
`timeout`. Text over 280 characters needs a verified account; the hard ceiling is 25,000.

### Not supported, deliberately

DMs are excluded. The opencli `reply-dm` command has no recipient argument and broadcasts to
the 20 most recent conversations; there is no per-user DM or inbox read. Do not use it.
The `accept` command, which accepts DM requests by keyword, is excluded for the same reason.

Retweet, bookmark and delete are not wired up. If the user requests one, use `opencli twitter`
directly and tell the user you are doing so.

## Tests

From the marketplace root:

```sh
node --test plugins/intel/skills/manage-x-account/tests/*.test.mjs
```
