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
