# Task notification delivery

Copied from real Claude Code transcript `e9f4ef31-ba68-4938-913e-cf4c9042911a.jsonl`, lines 926, 930, 969 and 971. Only unrelated top-level fields were removed; notification text and delivery metadata are unchanged.

The first enqueue is delivered as a `queued_command` attachment. The second enqueue is delivered as a user record with `turnOrigin: task_notification` and `origin.kind: task-notification`.
