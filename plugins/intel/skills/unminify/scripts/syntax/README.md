# Syntax rules

The CLI finishes syntax for every selected file before any rename plan or request.
`stage.mjs` persists independent checkpoints under `.unminify/syntax/`; rename
journals retain their source hashes, batch IDs and offsets. A prepared syntax write
can resume on either side of its atomic replacement. Files already owned by a rename
journal resume that journal without running syntax again. Start a separate run after
finishing existing journals to apply newly installed rules to those files.

## Add a rule

1. Add `rules/<rule>.mjs`, with a Wakaru v1.14.0 / Apache-2.0 provenance header and
   a default export `{ name, visitor(context) }`.
2. Add `../../tests/syntax-<rule>.test.mjs` with independently authored examples.
   Run the failing test before implementing the rule.
3. Add one `{ order, name, module: './rules/<rule>.mjs' }` line to `registry.mjs`.

Order numbers match Wakaru's registry. First-batch slots are SimplifySequence (2),
FlipComparisons (3), UnTypeofStrict (4), RemoveVoid (5), UnminifyBooleans (6),
UnDoubleNegation (7), UnInfinity (8), UnIndirectCall (9), UnTypeof (10),
UnNumericLiteral (11), UnStringEscape (12), UnBracketNotation (13), and
SimplifySequence2 (24). Register the same module again at 24 with a distinct step
name; implementation and tests remain shared. Execution sorts by order, without
deduplicating modules. Rule additions do not require changes to the CLI or runner.

## Rule contract

`visitor(context)` returns a Babel visitor. `context.source` is the current pass's
text, and `context.fragment` includes detected unbound exports. Parentheses have
explicit `ParenthesizedExpression` nodes. Every pass gets a fresh parse and scope.

`context.replace(pathOrNode, text)` records an original source interval. The runner
applies nonoverlapping edits, preserves every byte outside them, and inserts spaces
when a word replacement would merge with an adjacent token. A rule supplies the
parentheses and statement syntax required by its parent context. Nested edits that
overlap fail explicitly: choose a single enclosing replacement or skip the subtree.
AST mutation alone never writes output; the source edits are authoritative. Do not
print an entire file. Generating a replacement for a transformed node is allowed.

`context.canIntroduceGlobal(path, name)` authorizes a new intrinsic reference only
with no binding of that name anywhere in the current module, no `with`,
and no unknown direct eval or known eval mentioning that name. It also records the
name for this pass's independent free-variable check. Call it only for the exact
intrinsic reference being synthesized. A rule declares these names in its `globals`
array, e.g. `globals: ['undefined']`, so checkpoints track relevant global evidence.

For fragments, pass `fragmentContext` from `indexFragmentContext(files)` or
`loadFragmentContext(root, inputs)`. The latter indexes the whole Git root, falling
back to the supplied input files outside Git. The context includes the union of all
program binding names, including import aliases. An intrinsic is permitted only when
absent from that union; local bindings and dynamic-scope checks still apply. Direct
API calls without this context retain the conservative fragment policy. The CLI
passes the context to both syntax rewriting and its independent checks. Checkpoints
retain the shadowed intrinsic names rather than unrelated names that renaming may change.

Rules must skip transformations that change fragment Program bindings, declaration
kinds, declaration grouping or placement. Full local function transformations can
use complete Babel scopes. `checks.mjs` reparses both texts, verifies fragment
bindings and declaration layout, and rejects newly introduced free variables except
intrinsics explicitly authorized by the pass's guard. A rejected pass throws before
any file write. Existing rename checks are independent and unchanged.

`transformSyntax(source, { fragment, fragmentContext, rules })` returns `{ source, steps }`; tests may
supply rule objects directly. Each step records its name and accepted source edits.
The file checkpoint/status emits only edit counts, never source text.

## Parallel work

Each thread owns its rule module and test. Keep `runner.mjs`, `guards.mjs`,
`checks.mjs`, `stage.mjs`, the CLI and plugin manifests with one framework owner.
Registry rows are the only shared rule-addition edits; collect these with that
owner or integrate them sequentially. Global and declaration proof requirements
belong in the shared guards/checks, not duplicated rule-specific fallbacks.
