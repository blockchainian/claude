# claude

The `blockchainian` plugin marketplace for Claude Code, with Cloudflare, Web,
Proxy, Creator, Render, Mobile, Intel, and Secrets also available in Codex from the same repository.

| Plugin | What it does |
|---|---|
| [codex](plugins/codex/README.md) | Runs codex worker and review threads on the shared app-server daemon through the `codex-manager` MCP server: start, send, reply, interrupt, list, review, with an inbox that wakes Claude when a thread finishes or asks. |
| [feature](plugins/feature/README.md) | Ships a written plan: codex workstreams and UX agents in worktrees, merged behind the plan's checks, reviewed by codex, deployed and checked as the plan says; plus handoff and retro. |
| [mobile](plugins/mobile/README.md) | Builds, runs, profiles, and screenshots iOS apps on a simulator or a connected iPhone. |
| [render](plugins/render/README.md) | The Render plugin with only the nine skills this desk uses, plus its MCP server, agent, and hook. |
| [cloudflare](plugins/cloudflare/README.md) | The Cloudflare plugin with only the six skills this desk uses, plus its five MCP servers. |
| [proxy](plugins/proxy/README.md) | Captures and decodes the HTTP and WebSocket traffic of one target web or mobile app with mitmproxy, scoped to that app's hosts. |
| [secrets](plugins/secrets/README.md) | Manages local credentials and browser sessions through external app adapters. |
| [intel](plugins/intel/README.md) | Gathers and distills knowledge from long-form sources: transcribes audio locally with whisper, turns an article, podcast, video or PDF into searchable highlights, translates an English EPUB into a Chinese PDF, finds and downloads books, analyzes App Store reviews, finds brand names with a registrable domain, researches one creator into a sourced case study, and fetches and analyzes X, TikTok and app review archives. |
| [creator](plugins/creator/README.md) | Posts a video to the secrets-manager TikTok account through its own browser profile, fetches the account's video stats, lists the commercial music library's hottest sounds, and opens the account's browser for a person to look. |
| [computer-use](plugins/computer-use/README.md) | Captures one macOS app window to a PNG without hiding or moving the user's other windows. |
| [web](plugins/web/README.md) | Finds web memory leaks by diffing V8 heap snapshots captured from a running Chrome over the DevTools protocol, and checks a built page against a design reference. |

## Install

### Claude Code

```
/plugin marketplace add blockchainian/claude
/plugin install codex@blockchainian
/plugin install mobile@blockchainian
/plugin install render@blockchainian
/plugin install cloudflare@blockchainian
/plugin install proxy@blockchainian
/plugin install secrets@blockchainian
/plugin install intel@blockchainian
/plugin install web@blockchainian
/plugin install computer-use@blockchainian
/plugin install creator@blockchainian
```

Claude Code registers one marketplace per name, so adding this repository is
the only step; every plugin is then installable from it. Each plugin's own
README covers its requirements and usage.

### Codex

```sh
codex plugin marketplace add blockchainian/claude
codex plugin add cloudflare@blockchainian
codex plugin add web@blockchainian
codex plugin add proxy@blockchainian
codex plugin add creator@blockchainian
codex plugin add render@blockchainian
codex plugin add mobile@blockchainian
codex plugin add intel@blockchainian
codex plugin add secrets@blockchainian
```

The Codex catalog contains these eight plugins. For a local checkout, run
`codex plugin marketplace add .` from its root. Start a new session after installing.
These commands were verified with Codex CLI 0.160.0.

Intel exposes fourteen skills in Codex; its case-study Workflow remains Claude-only.
Each plugin's README covers its requirements and platform-specific behavior.
Skills resolve script paths from the loaded `SKILL.md` directory in each shell call.
For Mobile phone automation, review and trust `phone-session-gate` in `/hooks` first.

## Layout

```
.claude-plugin/marketplace.json   the catalog, listing every plugin
.agents/plugins/marketplace.json the Codex catalog, listing eight portable plugins
plugins/codex/                    the codex-manager plugin
plugins/feature/                  the ship / handoff / retro plugin
plugins/mobile/                   the iOS plugin
plugins/render/                   the Render plugin, trimmed to nine skills
plugins/cloudflare/               the Cloudflare plugin, trimmed to six skills
plugins/proxy/                    the mitmproxy traffic-capture plugin
plugins/intel/                    the research plugin: transcribe, digest, translate, case study and more
plugins/secrets/                  the account and session plugin
plugins/web/                      the web leak-finder and design-check plugin
plugins/computer-use/             the single-window capture plugin
plugins/creator/                  the creator-account plugin: TikTok upload and stats
tests/                            the marketplace node suite
```

Every plugin pins a `version` in `.claude-plugin/plugin.json`; Render and Intel also have
a `.codex-plugin/plugin.json` for host-specific settings. Render scopes OAuth and hooks; Intel lists fourteen
portable skills and excludes Claude-only case-study. Keep each plugin's manifest
versions equal. Bump the version in the same commit as
the change you want to ship — a plugin whose version is unchanged stays cached
on every machine that already has it, however much its code moved.

## Tests

```
npm test              # every suite
npm run test:codex    # codex-manager against a fake app-server daemon
npm run test:feature  # the feature plugin's hooks, plan checkers, workstream.sh, watch-ci and retro
npm run test:proxy    # the proxy plugin's capture logic and WireGuard key derivation
npm run test:intel    # the intel plugin's skills; the render suites need Chrome and uv
npm run test:web      # the heap-snapshot diff and the design check; needs uv
npm run test:mobile   # the phone-session hook, the simulator claim, frame diff, stitch and design check; needs uv
npm run test:secrets  # the local secrets engine and adapter contract
npm run test:creator  # the creator plugin's account pick, args, stats and sound rows
npm run test:marketplace  # the marketplace manifest and shared-engine checks
npm run validate      # the marketplace and plugin manifests
```

## License

MIT

## Plugin environment naming

Shared request settings have no plugin prefix (`RESIDENTIAL_PROXY_URL`, `X_BEARER_TOKEN`). Plugin data, output, configuration and state directories keep their owner (`INTEL_DATA_DIR`, `INTEL_OUTPUT_DIR`, `SECRETS_DATA_DIR`, `PROXY_CONFIG_DIR`, `CODEX_MANAGER_STATE_DIR`); all consumers refer to `SECRETS_DATA_DIR` for the shared credential store. Service names stay in settings such as `NAMECHEAP_API_KEY`, and the daemon target stays in `CODEX_DAEMON_SOCKET`. Names use uppercase snake case, `_DIR` for directories and `_ID` for identifiers. Timeout/interval values are in seconds as documented. Upstream CLI/host/MCP variables retain their required names. Internal shell variables and target-project application settings are not plugin configuration.

File-based configuration lives outside the checkout: Intel at `~/.config/intel/.env`, Secrets at `~/.config/secrets-manager/.env`, and Creator at `~/.config/creator/.env`. Each has one empty plugin-root `.env.example`. Creator’s configuration is independent while its accounts and browser profiles remain in Secrets Manager. Other plugins’ optional settings remain shell/MCP settings as documented in their environment tables.
