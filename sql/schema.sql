CREATE TABLE IF NOT EXISTS events
(
    id          INTEGER PRIMARY KEY,
    observed_at INTEGER NOT NULL,
    event_type  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS foreground_window_events
(
    event_id        INTEGER PRIMARY KEY,
    window_id       INTEGER NOT NULL,
    executable      TEXT    NOT NULL,
    executable_path TEXT,
    title           TEXT    NOT NULL,
    FOREIGN KEY (event_id) REFERENCES events (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS events_observed_at
    ON events (observed_at);

CREATE INDEX IF NOT EXISTS foreground_window_events_executable
    ON foreground_window_events (executable);

CREATE TABLE IF NOT EXISTS suggestions
(
    interval_start    INTEGER NOT NULL,
    interval_finish   INTEGER NOT NULL,
    interval_duration INTEGER NOT NULL,
    generated_at      INTEGER NOT NULL,
    jira_issue_key    TEXT,
    PRIMARY KEY (interval_start, interval_finish),
    CHECK (interval_start < interval_finish),
    CHECK (interval_finish - interval_start = interval_duration)
);

CREATE INDEX IF NOT EXISTS suggestions_interval_start
    ON suggestions (interval_start);
