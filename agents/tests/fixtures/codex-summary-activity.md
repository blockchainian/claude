# Summary append fixture

`codex-summary-activity.jsonl` is derived from `codex-latest-activity.jsonl`.
It keeps the first record of each record type and payload type, in source
order, with strings intact. Repeated permission entries and raw-response token
attribution items retain their first real entry. Summary parsing ignores these
collections; the full source remains covered by
`summary_cache_keeps_only_observed_fields_of_context_and_raw_responses`.
The copy exceeds the 64 KiB summary page size to exercise tail expansion, and
keeps few lines because the append tests re-parse the file once per line.

Regenerate from the repository root:

```sh
python3 - <<'PY'
import json
from pathlib import Path

fixtures = Path('agents/tests/fixtures')
source = fixtures / 'codex-latest-activity.jsonl'

kinds = {}
for line in source.read_text().splitlines():
    record = json.loads(line)
    kinds.setdefault((record['type'], record['payload'].get('type')), record)
records = list(kinds.values())
for record in records:
    payload = record['payload']
    if record['type'] == 'turn_context':
        for policy in (payload['permission_profile']['file_system'],
                       payload['file_system_sandbox_policy']):
            policy['entries'] = policy['entries'][:1]
    if record['type'] == 'event_msg' and payload['type'] == 'raw_response_completed':
        attribution = payload['usage_metadata']['metadata']['attribution']
        attribution['items'] = dict(list(attribution['items'].items())[:1])
text = ''.join(json.dumps(record, ensure_ascii=False, separators=(',', ':')) + '\n'
               for record in records)
assert len(text.encode()) > 64 * 1024
(fixtures / 'codex-summary-activity.jsonl').write_text(text)
print(len(records), len(text.encode()))
PY
```
