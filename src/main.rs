#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    use std::io;
    use std::time::{SystemTime, UNIX_EPOCH};

    use flogging::collectors::windows::WindowsCollector;
    use flogging::events::EventPayload;
    use flogging::events::store::EventStore;

    const DATABASE_PATH: &str = "flogging.db";

    let store = EventStore::open(DATABASE_PATH)?;
    let collection_started_at = SystemTime::now();
    let collector = WindowsCollector::start(store);

    println!("Collecting foreground-window events in {DATABASE_PATH}.");
    println!("Press Enter to stop.");

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    collector.shutdown()?;
    let collection_finished_at = SystemTime::now();

    let store = EventStore::open(DATABASE_PATH)?;
    let events = store.events_between(collection_started_at, collection_finished_at)?;
    let recent_events = &events[events.len().saturating_sub(10)..];

    println!(
        "Stored {} events during this session. Most recent:",
        events.len()
    );

    for event in recent_events {
        let observed_at = event.observed_at.duration_since(UNIX_EPOCH)?.as_millis();

        match &event.payload {
            EventPayload::ForegroundWindowObserved {
                window_id,
                executable,
                executable_path,
                title,
            } => {
                let executable_path = executable_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<executable path unavailable>".to_owned());

                println!(
                    "{observed_at} | window {window_id} | {title} | \
                     {executable} | {executable_path}"
                );
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("The flogging foreground-window collector currently runs only on Windows.");
}
