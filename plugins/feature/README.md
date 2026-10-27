# feature

The UX agents for a frontend workstream: `ux-implementer` implements it, `ux-autofixer` fixes the UX findings routed to it, and `ux-verifier` drives a scripted UI check and returns a verdict with evidence paths.

## Agents

| Agent | Model | What it does |
|---|---|---|
| `ux-implementer` | Fable medium | Implement one UX workstream in the checkout its brief names (the session tree or its own worktree), commit after every step, return flat JSON |
| `ux-autofixer` | Opus 5.5 medium | Fix the PR threads labelled `claude-code-ux` in the UX workstream's worktree, push, reply, resolve |
| `ux-verifier` | Sonnet low | Drive a scripted UI scenario (browse or iOS simulator) and return a verdict with evidence paths |

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install feature@blockchainian
```

## License

MIT
