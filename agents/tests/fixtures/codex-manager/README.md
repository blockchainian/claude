# codex-manager captures

`attached-*` and `detached-*` were copied from local codex-manager reverse
ownership records and their matching `state.json` entries. Only the matching
thread is retained; identifiers, names, paths and process ids are scrubbed.
The attached capture has no `detached` field; the detached capture has
`detached: true` and `owner: "main"`.

No pending approvals or ask files existed during capture. Tests exercise their
creation and removal using the formats written by `manager.mjs` and `inbox.mjs`.
