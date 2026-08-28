pub mod store;

use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub interval_start: SystemTime,
    pub interval_finish: SystemTime,
    pub generated_at: SystemTime,
    pub jira_issue_key: Option<String>,
}

impl Suggestion {
    pub fn new(
        interval_start: SystemTime,
        interval_finish: SystemTime,
        generated_at: SystemTime,
        jira_issue_key: Option<String>,
    ) -> Self {
        Self {
            interval_start,
            interval_finish,
            generated_at,
            jira_issue_key,
        }
    }
}
