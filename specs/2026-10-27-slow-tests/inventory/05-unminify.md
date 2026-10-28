# unminify tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 85.14 | hundreds of files bound planning through writeback while filling request slots and resuming | plugins/intel/skills/unminify/tests/files-memory.test.mjs |
| 2 | 23.54 | USAGE_LIMIT follows imports only in done files and resumes byte-identically | plugins/intel/skills/unminify/tests/imports-resume.test.mjs |
| 3 | 21.10 | many real batches fill a local HTTP/2 session and persist valid rewrites | plugins/intel/skills/unminify/tests/render-performance.test.mjs |
| 4 | 19.82 | module batches in either dependency order match whole-directory phases byte for byte | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 5 | 18.49 | AUTH follows imports only in done files and resumes byte-identically | plugins/intel/skills/unminify/tests/imports-resume.test.mjs |
| 6 | 15.12 | sync-imports resumes after a committed chunk before ordinary writes byte-identically | plugins/intel/skills/unminify/tests/import-sync-parallel.test.mjs |
| 7 | 14.61 | chunk program naming commits one complete reply after validation | plugins/intel/skills/unminify/tests/output-cap.test.mjs |
| 8 | 13.42 | sync-imports recovers a real SIGKILL between chunk member replacements | plugins/intel/skills/unminify/tests/import-sync-parallel.test.mjs |
| 9 | 12.73 | C: separate selections equal one run byte for byte | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 10 | 12.62 | real CLI merges before naming, checkpoints and resumes chunks and standalone files | plugins/intel/skills/unminify/tests/merge-imports.test.mjs |
| 11 | 12.29 | real restore-syntax matches captured 0.71.3 source and journal bytes and resumes unchanged | plugins/intel/skills/unminify/tests/fragment-index-parallel.test.mjs |
| 12 | 12.19 | A: separate selections equal one run byte for byte | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 13 | 11.93 | rename-bindings HTTP request count matches a chunk containing only selected declarations | plugins/intel/skills/unminify/tests/rename-scope.test.mjs |
| 14 | 11.50 | probe scenarios 1 and 2: sibling-only relative imports follow identically together and in batches | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 15 | 11.32 | C-shared: separate selections equal one run byte for byte | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 16 | 11.02 | a busy caller cannot time out a valid module syntax check | plugins/intel/skills/unminify/tests/check-timeout.test.mjs |
| 17 | 9.38 | three separate phases match the actual pre-split run byte for byte and each resumes | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 18 | 9.31 | sync-imports uses several real workers and preserves captured 0.71.4 source and journal bytes | plugins/intel/skills/unminify/tests/import-sync-parallel.test.mjs |
| 19 | 9.11 | chunk guard checks cross-file and nested renames within a bounded worker heap | plugins/intel/skills/unminify/tests/chunk-check-memory.test.mjs |
| 20 | 8.94 | rename-bindings names only selected declarations and follows their sibling references | plugins/intel/skills/unminify/tests/rename-scope.test.mjs |
| 21 | 8.37 | parallel CLI preserves serial source bytes and syntax and chunk journal bytes | plugins/intel/skills/unminify/tests/syntax-parallel.test.mjs |
| 22 | 8.22 | real CLI completes syntax, strict rename checks, and a completed resume without credentials | plugins/intel/skills/unminify/tests/syntax-cli.test.mjs |
| 23 | 7.87 | SIGKILL after a durable chunk syntax write resumes to uninterrupted source and journal bytes | plugins/intel/skills/unminify/tests/syntax-parallel.test.mjs |
| 24 | 7.64 | an incomplete reply leaves no partial journal reply and resumes the original batch | plugins/intel/skills/unminify/tests/output-cap.test.mjs |
| 25 | 7.26 | saved chunk top-level replies survive SIGKILL and resume identically | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 26 | 7.01 | SIGKILL after N replies resumes to byte-identical output | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 27 | 6.99 | chunk duplicate declarations freeze without merging unrelated same-name chunks | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 28 | 6.99 | chunk analyses are bounded independently of request concurrency and completed chunks resume | plugins/intel/skills/unminify/tests/chunk-memory.test.mjs |
| 29 | 6.80 | an invalid reply leaves no partial journal reply and resumes the original batch | plugins/intel/skills/unminify/tests/output-cap.test.mjs |
| 30 | 6.59 | CLI restores sibling-only external imports | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 31 | 6.40 | ordinary historical ignored consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 32 | 6.37 | history without a manifest retains recorded sibling authorization | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 33 | 6.28 | all refreshes completed worker history when an unfinished selection expands | plugins/intel/skills/unminify/tests/worker-selection.test.mjs |
| 34 | 6.19 | real CLI finishes all syntax writes before rename planning and resumes offsets | plugins/intel/skills/unminify/tests/syntax-cli.test.mjs |
| 35 | 6.12 | same-file small function batches share bounded requests (1600 tokens) | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 36 | 6.10 | perf summarizes recorded local HTTP/2 run without model calls | plugins/intel/skills/unminify/tests/perf.test.mjs |
| 37 | 5.93 | sibling import authorization: selectedReference, current and historical selections | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 38 | 5.89 | large independent chunks execute off the main thread and reuse durable replies | plugins/intel/skills/unminify/tests/rename-workers.test.mjs |
| 39 | 5.79 | real CLI uses siblings outside its selection, shadowed=true | plugins/intel/skills/unminify/tests/syntax-context.test.mjs |
| 40 | 5.65 | quota interruption resumes only unrecorded original batch ids | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 41 | 5.61 | one shared namer, stage ordering, bounded concurrency, completion replay and usage | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 42 | 5.54 | same-file small function batches share bounded requests (1000 tokens) | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 43 | 5.43 | AUTH drains in-flight replies, persists them and resumes | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 44 | 5.16 | USAGE_LIMIT drains in-flight replies, persists them and resumes | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 45 | 4.91 | selection uses multiple real parse workers and preserves the serial file lists and ignored sibling | plugins/intel/skills/unminify/tests/selection-parallel.test.mjs |
| 46 | 4.90 | ordinary import merging has its own summary phase outside selection_plan | plugins/intel/skills/unminify/tests/selection-parallel.test.mjs |
| 47 | 4.83 | file duration ends at its main write while import writes are reported separately | plugins/intel/skills/unminify/tests/perf.test.mjs |
| 48 | 4.83 | restore-syntax expands chunk siblings, filters ignored and invalid files, and resumes without naming or imports | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 49 | 4.80 | chunk import aliases follow an exporter without a model request and update sibling uses | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 50 | 4.79 | CLI replay protects a sibling name outside its input and still renames a local | plugins/intel/skills/unminify/tests/fragments.test.mjs |
| 51 | 4.77 | an unfinished chunk resumes saved program replies after changing selected file | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 52 | 4.77 | ordinary partial replies write and replay with missing bindings retained | plugins/intel/skills/unminify/tests/partial-names.test.mjs |
| 53 | 4.69 | rewrite failures preserve originals and import following is independently gated | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 54 | 4.68 | sibling import authorization: conflict, current and historical selections | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 55 | 4.68 | sibling import authorization: external, current and historical selections | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 56 | 4.58 | whole CLI run resolves Git root exactly once through real git subprocesses | plugins/intel/skills/unminify/tests/selection-parallel.test.mjs |
| 57 | 4.51 | ordinary historical changed consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 58 | 4.48 | CLI file external aliases honor threshold 4 without model requests | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 59 | 4.45 | empty original batches stay pending while packed peers are journaled (chunk=true) | plugins/intel/skills/unminify/tests/partial-names.test.mjs |
| 60 | 4.39 | sibling import authorization: ignored, current and historical selections | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 61 | 4.37 | file external collisions use source prefixes and reach a fixed point | plugins/intel/skills/unminify/tests/external-import-prefix.test.mjs |
| 62 | 4.33 | selection changes replay nonempty saved program names without another program request | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 63 | 4.32 | quota stop journals accepted batches from its own unfinished stage before returning | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 64 | 4.19 | real CLI uses siblings outside its selection, shadowed=false | plugins/intel/skills/unminify/tests/syntax-context.test.mjs |
| 65 | 4.18 | placeholders from other packed batches cannot shadow imported context names | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 66 | 4.15 | ignored sibling references freeze even with expansion | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 67 | 4.12 | chunk external collisions use source prefixes and reach a fixed point | plugins/intel/skills/unminify/tests/external-import-prefix.test.mjs |
| 68 | 4.10 | ordinary historical unfinished consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 69 | 4.10 | real CLI executes the three commands separately without credentials or model requests | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 70 | 4.05 | sibling import authorization: unchanged, current and historical selections | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 71 | 3.97 | empty original batches stay pending while packed peers are journaled (chunk=false) | plugins/intel/skills/unminify/tests/partial-names.test.mjs |
| 72 | 3.95 | one run renames the files of every given path once | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 73 | 3.93 | a failed NETWORK batch leaves its file unwritten and unfinished so resume retries it | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 74 | 3.93 | planning and packing a synthetic 150KB src file take seconds | plugins/intel/skills/unminify/tests/render-performance.test.mjs |
| 75 | 3.84 | a frozen duplicate import alias does not abort import following | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 76 | 3.79 | CLI chunk external aliases honor threshold 8 without model requests | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 77 | 3.75 | historical completed chunks follow imports even without a manifest on the later run | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 78 | 3.74 | collision asks for a contextual distinct name and resumes (chunk, local=true, retry=readLimit) | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 79 | 3.71 | chunk collision suffixes reach a fixed point when restoring external aliases | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 80 | 3.64 | collision asks for a contextual distinct name and resumes (chunk, local=false, retry=readLimit) | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 81 | 3.61 | sync-imports updates previously named files beyond its selected path | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 82 | 3.57 | ignore uses repository root and gitignore directory, glob, negation, comments, for directories and files | plugins/intel/skills/unminify/tests/select.test.mjs |
| 83 | 3.53 | CLI chunk external aliases honor threshold 4 without model requests | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 84 | 3.51 | CLI file external aliases honor threshold 8 without model requests | plugins/intel/skills/unminify/tests/external-imports.test.mjs |
| 85 | 3.42 | collision asks for a contextual distinct name and resumes (file, local=false, retry=readCount) | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 86 | 3.40 | CLI resumes between the syntax, merge and rename checkpoints without a model | plugins/intel/skills/unminify/tests/merge-imports.test.mjs |
| 87 | 3.37 | chunk import aliases stay unchanged when their exporting binding did not change | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 88 | 3.35 | chunk local partial replies pass chunk validation and preserve missing local names | plugins/intel/skills/unminify/tests/partial-names.test.mjs |
| 89 | 3.34 | chunk historical unfinished consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 90 | 3.29 | chunk historical ignored consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 91 | 3.28 | separate files never share a model request | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 92 | 3.25 | collision asks for a contextual distinct name and resumes (file, local=false, retry=readLimit) | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 93 | 3.14 | chunk historical changed consumers are never written | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 94 | 3.12 | probe scenario 3: selecting all chunk members still follows | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 95 | 3.12 | completed rejected files remain visible as failures on resume | plugins/intel/skills/unminify/tests/cli.test.mjs |
| 96 | 3.09 | default parameter collision completes through the local service (chunk) | plugins/intel/skills/unminify/tests/naming-pattern-scope.test.mjs |
| 97 | 3.05 | sample runner executes all three CLI stages for an authored credential-free sample | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 98 | 3.02 | large optional context is shortened and every request includes prompt overhead | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 99 | 2.99 | chunk exports declared in a sibling propagate through another chunk to a standalone importer | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 100 | 2.91 | parse workers return only parseability and parse time, and callbacks exclude invalid files | plugins/intel/skills/unminify/tests/selection-parallel.test.mjs |
| 101 | 2.87 | missing, unknown and removed path-first commands print usage and fail | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 102 | 2.79 | default budget keeps hundreds of small functions in a single request | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 103 | 2.64 | chunk naming asks once, renders sibling use, preserves export and replays completion | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 104 | 2.53 | chunk selection expands to siblings | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 105 | 2.51 | all preserves a rejected worker journal and its external import alias | plugins/intel/skills/unminify/tests/worker-selection.test.mjs |
| 106 | 2.50 | parameter collision retry includes its owning function context | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 107 | 2.45 | chunks execute in separate workers, preserve semantics, and resume without requests | plugins/intel/skills/unminify/tests/chunk-workers.test.mjs |
| 108 | 2.43 | without --chunks the legacy fragment protection remains in force | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 109 | 2.40 | a model failure in a later chunk selection exits with the selection still pending | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 110 | 2.40 | fragment task queue is bounded and no-Git fallback preserves input order | plugins/intel/skills/unminify/tests/fragment-index-parallel.test.mjs |
| 111 | 2.39 | an irreducible collision budget failure leaves its chunk resumable and runs the next chunk | plugins/intel/skills/unminify/tests/chunk-budget-isolation.test.mjs |
| 112 | 2.39 | default parameter collision completes through the local service (file) | plugins/intel/skills/unminify/tests/naming-pattern-scope.test.mjs |
| 113 | 2.27 | outside chunk import declarations follow in sibling chunk members | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 114 | 2.26 | a journal error still follows completed imports without changing the unfinished file | plugins/intel/skills/unminify/tests/imports-resume.test.mjs |
| 115 | 2.23 | rename-bindings lists every unrestored selected file and chunk sibling before any request | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 116 | 2.22 | chunk syntax is committed before naming while import following waits for validation | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 117 | 2.21 | retry-created collisions also ask once per binding | plugins/intel/skills/unminify/tests/naming-collision-retry.test.mjs |
| 118 | 2.19 | fragment workers read paths and return plain per-file facts without source or syntax trees | plugins/intel/skills/unminify/tests/fragment-index-parallel.test.mjs |
| 119 | 1.97 | dynamic chunk bindings and locals remain unchanged without model requests | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 120 | 1.96 | sync-imports expands newly selected chunk siblings without restoring syntax or naming | plugins/intel/skills/unminify/tests/split-commands.test.mjs |
| 121 | 1.96 | single-batch requests preserve their original rendered context | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 122 | 1.83 | syntax pool map drains running file tasks before reporting failure | plugins/intel/skills/unminify/tests/import-workers.test.mjs |
| 123 | 1.82 | probe scenario 4: unselected ordinary consumers stay untouched | plugins/intel/skills/unminify/tests/sibling-follow.test.mjs |
| 124 | 1.81 | duplicate import aliases remain frozen when the exporter really changes | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 125 | 1.81 | chunk unfinished exports remain readable without any write | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 126 | 1.77 | fragment index uses multiple real workers and matches serial context including insertion order | plugins/intel/skills/unminify/tests/fragment-index-parallel.test.mjs |
| 127 | 1.70 | file selection includes final fragments with undeclared exports | plugins/intel/skills/unminify/tests/fragments.test.mjs |
| 128 | 1.69 | CLI completes a real prepared half-write before rebuilding chunk plans | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 129 | 1.64 | a real syntax gate rejects the entire chunk including pending syntax changes | plugins/intel/skills/unminify/tests/chunk-cli.test.mjs |
| 130 | 1.54 | ordinary unfinished exports remain readable without any write | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 131 | 1.54 | unfinished import/export intermediaries expose only the names actually on disk | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 132 | 1.44 | ignored import/export intermediaries expose only the names actually on disk | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 133 | 1.33 | an irreducible batch over budget is rejected before HTTP and preserves source | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 134 | 1.25 | real syntax workers match serial transforms and whole-chunk import merges byte for byte | plugins/intel/skills/unminify/tests/syntax-parallel.test.mjs |
| 135 | 1.22 | an unfinished journal with removed batch options is refused | plugins/intel/skills/unminify/tests/packing.test.mjs |
| 136 | 1.13 | USAGE_LIMIT still stops queued chunks through the local model transport | plugins/intel/skills/unminify/tests/chunk-budget-isolation.test.mjs |
| 137 | 1.08 | pipeline checkpoints are not historical rename journals | plugins/intel/skills/unminify/tests/batch-follow.test.mjs |
| 138 | 1.05 | rename syntax preflight uses the same real transform and chunk merge workers without writing source | plugins/intel/skills/unminify/tests/syntax-parallel.test.mjs |
