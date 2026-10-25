-- Schema copied from sqlite3 -readonly state_5.sqlite .schema; rows sampled from that database.
-- Identifiers, user text, and paths scrubbed. Tests build a temporary SQLite database.
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE TABLE thread_sections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
, appearance TEXT);
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    rollout_path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    source TEXT NOT NULL,
    model_provider TEXT NOT NULL,
    cwd TEXT NOT NULL,
    title TEXT NOT NULL,
    sandbox_policy TEXT NOT NULL,
    approval_mode TEXT NOT NULL,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    has_user_event INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    archived_at INTEGER,
    git_sha TEXT,
    git_branch TEXT,
    git_origin_url TEXT
, cli_version TEXT NOT NULL DEFAULT '', first_user_message TEXT NOT NULL DEFAULT '', agent_nickname TEXT, agent_role TEXT, memory_mode TEXT NOT NULL DEFAULT 'enabled', model TEXT, reasoning_effort TEXT, agent_path TEXT, created_at_ms INTEGER, updated_at_ms INTEGER, thread_source TEXT, preview TEXT NOT NULL DEFAULT '', recency_at INTEGER NOT NULL DEFAULT 0, recency_at_ms INTEGER NOT NULL DEFAULT 0, history_mode TEXT NOT NULL DEFAULT 'legacy', name TEXT, is_pinned INTEGER NOT NULL DEFAULT 0, thread_section_id TEXT
    REFERENCES thread_sections(id) ON DELETE SET NULL, section_position INTEGER, section_entered_at_ms INTEGER, project_id TEXT
    REFERENCES projects(id) ON DELETE SET NULL, originator TEXT, daybreak_enabled BOOLEAN, creator_user_id TEXT, creator_account_id TEXT);
CREATE TABLE thread_spawn_edges (
    parent_thread_id TEXT NOT NULL,
    child_thread_id TEXT NOT NULL PRIMARY KEY,
    status TEXT NOT NULL
);
INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,tokens_used,has_user_event,archived,archived_at,git_sha,git_branch,git_origin_url,cli_version,first_user_message,agent_nickname,agent_role,memory_mode,model,reasoning_effort,agent_path,created_at_ms,updated_at_ms,thread_source,preview,recency_at,recency_at_ms,history_mode,name,is_pinned,thread_section_id,section_position,section_entered_at_ms,project_id,originator,daybreak_enabled,creator_user_id,creator_account_id) VALUES ('01a14609-1ff3-7440-9ae8-013cd6baa4c1','01a14609-1ff3-7440-9ae8-013cd6baa4c1.jsonl',1792176365,1792176365,'vscode','openai','/workspace/example.com','提示词挪到最上','{}','on-request',4186881,0,0,NULL,NULL,'example.com',NULL,'0.162.1','Check the dashboard.',NULL,NULL,'enabled','gpt-6.1-sol','medium',NULL,1792176365556,1792176365556,NULL,'Check the dashboard.',1792176365,1792176365962,'paginated','提示词挪到最上',0,NULL,NULL,NULL,NULL,'codex-tui',NULL,NULL,NULL);
INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,tokens_used,has_user_event,archived,archived_at,git_sha,git_branch,git_origin_url,cli_version,first_user_message,agent_nickname,agent_role,memory_mode,model,reasoning_effort,agent_path,created_at_ms,updated_at_ms,thread_source,preview,recency_at,recency_at_ms,history_mode,name,is_pinned,thread_section_id,section_position,section_entered_at_ms,project_id,originator,daybreak_enabled,creator_user_id,creator_account_id) VALUES ('01a1460c-1776-7fb3-b058-d76e6b53fd1f','01a1460c-1776-7fb3-b058-d76e6b53fd1f.jsonl',1792176559,1792176559,'vscode','openai','/workspace/example.com','用量图颜色','{}','on-request',1677665,0,0,NULL,NULL,'example.com',NULL,'0.162.1','Check the dashboard.',NULL,NULL,'enabled','gpt-6.1-sol','medium',NULL,1792176559991,1792176559991,NULL,'Check the dashboard.',1792176560,1792176560391,'paginated','用量图颜色',0,NULL,NULL,NULL,NULL,'codex-tui',NULL,NULL,NULL);
