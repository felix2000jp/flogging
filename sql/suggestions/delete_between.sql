DELETE
FROM suggestions
WHERE interval_start >= ?1
  AND interval_start < ?2;
