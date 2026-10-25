Source: the real synthetic n2031-strict Responses capture, response ID
`resp_0656501d7b7b1a23016ada352d3c448190995495c87edd2608`.
The input was an authored sentence requesting 2031 placeholder names.

The fixture retains the terminal response.incomplete event's id, status, error,
incomplete_details and usage unchanged. Output text retains the first two complete
pairs and the original final `,"v2031` suffix; the intervening v3 through v2030
pairs are removed. No closing quote, value or object delimiter is added. Other
response and message metadata are omitted. No third-party source code or names
inferred from third-party code are included.

HTTP/2 tests replay the envelope and synthesize output deltas from that text.
Boundary tests deliberately alter the text or reason. Pipeline tests renumber
v2031 to v3 to match rendered binding requests. Supplement successes use a
completed SSE response from the local transport helper.
