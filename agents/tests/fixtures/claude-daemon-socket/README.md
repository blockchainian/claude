# Claude daemon protocol captures

Real Claude Code 2.1.309 captures using daemon protocol 1:

- `protocol-roundtrip.json`: captured request and response lines, including their line delimiters. Authentication is the existing `<redacted>` placeholder.
- `results.json`: captured successful acknowledgements for idle, busy and slash input. The tests serialize the captured `ack` objects; these are summaries, not original wire lines.
- `extra-results.json`: captured structured-input refusal and successful target-field and effort acknowledgements, also recorded as summary objects.

Malformed and missing-field test cases explicitly mutate these captured objects. Error-code coverage mutates the captured ENOJOB refusal. No daemon is simulated and no control key is read.
