SELECT events.observed_at,
       foreground_window_events.window_id,
       foreground_window_events.executable,
       foreground_window_events.executable_path,
       foreground_window_events.title
FROM events
         JOIN foreground_window_events
              ON foreground_window_events.event_id = events.id
WHERE events.event_type = ?1
  AND events.observed_at >= ?2
  AND events.observed_at < ?3
ORDER BY events.observed_at, events.id;
