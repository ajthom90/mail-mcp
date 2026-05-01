CREATE TABLE accounts (
    id              TEXT PRIMARY KEY,
    label           TEXT NOT NULL,
    provider        TEXT NOT NULL CHECK (provider IN ('gmail', 'm365', 'imap')),
    email           TEXT NOT NULL,
    config_json     TEXT NOT NULL DEFAULT '{}',
    scopes_json     TEXT NOT NULL DEFAULT '[]',
    created_at      INTEGER NOT NULL,
    last_validated  INTEGER
);

CREATE TABLE permissions (
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    category    TEXT NOT NULL CHECK (category IN ('read', 'modify', 'trash', 'draft', 'send')),
    policy      TEXT NOT NULL CHECK (policy IN ('allow', 'confirm', 'session', 'draftify', 'block')),
    PRIMARY KEY (account_id, category)
);

CREATE TABLE app_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
