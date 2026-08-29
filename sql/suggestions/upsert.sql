INSERT INTO suggestions (interval_start,
                         interval_finish,
                         interval_duration,
                         generated_at,
                         jira_issue_key)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT (interval_start, interval_finish) DO UPDATE
    SET interval_duration = excluded.interval_duration,
        generated_at   = excluded.generated_at,
        jira_issue_key = excluded.jira_issue_key;
