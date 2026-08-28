INSERT INTO suggestions (interval_start,
                         interval_finish,
                         generated_at,
                         jira_issue_key)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT (interval_start, interval_finish) DO UPDATE
    SET generated_at   = excluded.generated_at,
        jira_issue_key = excluded.jira_issue_key;
