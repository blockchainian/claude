# Summary append fixture

`codex-summary-activity.jsonl` is derived from `codex-latest-activity.jsonl`.
All 156 records, timestamps, types, IDs and ordering are retained. Strings are
limited to 512 characters; tool arguments and inputs remain intact. Repeated
permission entries and raw-response token attribution items retain their first
real entry. Summary parsing ignores these collections; the full source remains
covered by `summary_cache_keeps_only_observed_fields_of_context_and_raw_responses`.
The copy exceeds the 64 KiB summary page size to exercise tail expansion.

Regenerate from the repository root:

```sh
python3 - <<'PY'
import json
from pathlib import Path

fixtures = Path('agents/tests/fixtures')
source = fixtures / 'codex-latest-activity.jsonl'

def shorten(value, key=''):
    if isinstance(value, dict):
        return {k: shorten(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [shorten(v, key) for v in value]
    if isinstance(value, str) and key not in ('arguments', 'input'):
        return value[:512]
    return value

records = [shorten(json.loads(line)) for line in source.read_text().splitlines()]
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
