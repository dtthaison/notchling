CREATE TABLE conversation_summaries (
    conversation_id TEXT,
    title TEXT,
    preview TEXT,
    workspace_uris TEXT,
    status TEXT,
    last_modified_time TEXT,
    nesting_depth INTEGER,
    parent_conversation_id TEXT,
    agent_name TEXT,
    killed INTEGER
);
INSERT INTO conversation_summaries VALUES
    ('b0000000-0000-4000-8000-00000000000a', 'Split the review', '', '["file:///work/app"]', 'CASCADE_RUN_STATUS_RUNNING', '2026-11-13 09:10:00.000000+00:00', 0, NULL, NULL, 0),
    ('b0000000-0000-4000-8000-00000000000b', '', '', '["file:///work/app"]', 'CASCADE_RUN_STATUS_RUNNING', '2026-11-13 09:09:00.000000+00:00', 1, 'b0000000-0000-4000-8000-00000000000a', 'self', 0),
    ('b0000000-0000-4000-8000-00000000000c', '', '', '["file:///work/app"]', 'CASCADE_RUN_STATUS_IDLE', '2026-11-13 09:05:00.000000+00:00', 1, 'b0000000-0000-4000-8000-00000000000a', 'research', 0);
