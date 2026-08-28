SELECT interval_start,
       interval_finish,
       generated_at,
       jira_issue_key
FROM suggestions
WHERE interval_start >= ?1
  AND interval_start < ?2
ORDER BY interval_start, interval_finish;
