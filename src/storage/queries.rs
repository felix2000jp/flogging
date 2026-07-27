pub const SCHEMA: &str = include_str!("../../sql/schema.sql");

pub mod events {
    pub const INSERT: &str = include_str!("../../sql/events/insert.sql");
}

pub mod foreground_window_events {
    pub const INSERT: &str = include_str!("../../sql/foreground_window_events/insert.sql");
    pub const SELECT_ALL: &str = include_str!("../../sql/foreground_window_events/select_all.sql");
}
