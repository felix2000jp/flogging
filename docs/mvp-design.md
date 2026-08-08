# flogging MVP

Status: implementation in progress
Primary target: Windows 11 without administrator rights  
Language: Rust  
Interface: terminal UI

## 1. Goal

flogging passively records objective work events while it is running and reconstructs them into a useful calendar of the workday.

The MVP is successful when it can run during a normal workday, preserve the collected events locally, and produce a daily calendar that is useful enough to understand where the day went. It does not need perfect classification, but it should identify Jira tasks when the available evidence supports them and otherwise describe the observed activity.

The MVP consists of four parts:

1. **Collectors** observe Windows and Git events.
2. **Events** defines and preserves those facts in SQLite.
3. **Calendar** converts events into a daily calendar.
4. **TUI** displays the calendar and keeps it refreshed.

All four parts run in one process for the MVP.

## 2. Runtime model

The user starts flogging from a terminal at the beginning of the workday and leaves it running. Collection continues while the application is open.

```text
Windows collector ------> EventStore ------> SQLite
Git collector ----------> EventStore

main: EventStore query --> calendar::build(events) --> Calendar --> TUI
```

`main.rs` constructs the components and coordinates their lifecycles. It starts
collectors, queries the event store for the selected date, asks the calendar
module to build a `Calendar`, and gives that value to the TUI. These components
remain modules within one Rust crate so their responsibilities can later be
reused by a backend process with a different interface.

## 3. Collectors

Collectors observe facts and append typed events to the `EventStore`. They do not build calendar blocks, assign duration, or decide which activity is more important.

Each collector:

- starts when the application starts;
- stops when the application exits;
- records only meaningful changes rather than continuously recording unchanged state;
- reports failures without terminating the other collectors;
- uses the same event envelope and storage interface;
- records when each event was observed.

Collectors write through `EventStore` methods. They do not execute SQL directly. There is no global event queue or dedicated database-writer process in the MVP. The store is responsible for safe, short SQLite operations when collectors write concurrently.

A platform callback that cannot block may use a small collector-local handoff internally. That is an implementation detail of the collector, not part of the application architecture.

### 3.1 Windows collector

The Windows collector provides the main duration evidence for the MVP.

It records:

- foreground-window changes;
- process identity and window title;
- the beginning and end of idle periods;
- workstation lock and unlock when available through an appropriate safe Rust interface;
- application session boundaries or heartbeats needed to prevent an activity from appearing to continue after an unclean exit.

The idle threshold is 15 minutes. An idle event records the actual last-input time as well as when flogging detected the transition.

The collector must work as a normal Windows 11 user without elevation. Missing information from elevated or protected processes is accepted and should not stop collection.

IntelliJ, VS Code, Edge, Microsoft Teams, and Windows Terminal are initially observed through their foreground-window process and title. They are not separate MVP collectors. Recognition of a repository, Jira task, GitHub pull request, or generic Teams activity from that information belongs to calendar reconstruction.

### 3.2 Git collector

The Git collector watches repositories below configured source roots.

It records:

- repository discovery;
- branch changes;
- HEAD changes;
- newly observed commits and their basic metadata.

Branches such as `jofe/MBM-1111`, `jofe/MBFS-11111`, and `jofe/FCA-444` provide Jira-task evidence. Multiple repositories may support the same Jira task.

A checked-out branch is context, not by itself proof of continuous activity. Calendar reconstruction combines Git state with duration evidence such as a focused IDE or terminal associated with that repository. Branch and commit events are still preserved even when they do not produce a calendar block.

Switching to `main` or `master` and later returning to the task branch is recorded exactly as observed. Reconstruction rules, rather than the collector, decide how that affects the calendar.

## 4. Storage

Storage is represented by an in-process `EventStore` inside the `events` module. It is not a server, actor, or separate operating-system process.

The event store:

- initializes the local SQLite schema;
- appends one event or a small batch of events;
- reads events for a requested time range;
- stores collector checkpoints where a collector needs them;
- owns SQLite-specific details so other components do not depend on the schema.

Raw events are append-only and are the source of truth. Reconstruction never modifies or deletes them.

Every event has a common envelope:

```text
Event
  observed_at         time flogging observed the fact
  payload             event-specific data
```

SQLite assigns each stored event a local, monotonically increasing `id`.
`observed_at` is stored as UTC Unix milliseconds. When a source provides another
meaningful timestamp, it belongs to that event's payload. For example, a Git
commit payload records its commit timestamp.

The database lives below:

```text
%LOCALAPPDATA%\flogging\
```

For the MVP, the runtime calculates the requested day's event-query boundaries
using the computer's current system timezone. This is optimized for building
today's calendar. If the system timezone later changes, a historical calendar
may display its events in the new timezone; this tradeoff is accepted for the
MVP.

Calendar blocks are not persisted in the MVP. They are derived views of the raw events.

## 5. Calendar

The calendar module owns the `Calendar` and `CalendarBlock` types together with
the rules that reconstruct a calendar from events. It does not open or query an
`EventStore`.

Its primary operation is conceptually:

```text
calendar::build(NaiveDate, &[Event]) -> Calendar
```

The caller is responsible for querying the events within the requested local
date. For each build, the calendar module:

1. orders the supplied events deterministically;
2. reconstructs duration intervals from foreground, idle, lock, and application-lifecycle evidence;
3. recognizes contexts such as Jira tasks, repositories, GitHub pull requests, and generic application activity;
4. groups continuous evidence for the same context;
5. promotes occurrences lasting at least five minutes to calendar blocks;
6. reconciles blocks from the available event sources;
7. returns a complete, internally valid `Calendar`.

The requested day is rebuilt from its raw events rather than processing only
previously unseen events. The expected event volume for one day is small, and a
complete rebuild naturally handles late or out-of-order observations and
changed rules.

The application does not maintain a shared in-memory calendar store in the MVP.
The returned `Calendar` is a value owned by the runtime and passed to the TUI.

### 5.1 Context and block rules

An event may support more than one context. Calendar reconstruction does not use a priority list that forces all evidence into a single winner.

Examples:

- a focused IntelliJ window associated with a repository on `jofe/MBM-1111` supports coding work for Jira task `MBM-1111`;
- a focused GitHub pull-request page supports a code-review context;
- if that pull request also contains a Jira key, it may support the PR and Jira contexts simultaneously;
- focused Teams activity can become a generic descriptive block even though the MVP cannot reliably identify the scheduled meeting;
- generic focused activity without an inferred task can become a descriptive application block.

A pull-request review does not override a Jira task. Each context is reconstructed from its own evidence. Overlaps are valid calendar data even though the first TUI may show them in a simple form.

Point events such as commits enrich a context but do not prove continuous work between commits. Duration comes from interval evidence.

Occurrences shorter than five minutes remain available as raw events but do not become calendar blocks.

The same input events and rule version must always produce the same calendar.

## 6. TUI

The TUI displays one daily calendar. It renders `Calendar` values but does not
query events or reconstruct blocks.

It:

- displays today's calendar immediately after the runtime builds it on startup;
- keeps the last successfully returned `Calendar` as its display state;
- reports manual-refresh and selected-date actions to the runtime;
- displays collection or reconstruction errors without discarding the last good calendar;
- provides enough event detail to diagnose obviously incorrect reconstruction.

`main.rs` rebuilds today's calendar every five minutes and after a manual
refresh or selected-date change. Historical dates do not need periodic
refreshes.

Calendar data may contain overlapping blocks. The MVP TUI may use a simple overlap marker, selection, stacking, or detail view. The underlying calendar must preserve all supported blocks; the presentation does not determine the domain model.

The first view may use five-minute rows while retaining the actual timestamps in the returned calendar.

## 7. Project structure

Use one Cargo package with one library crate and a thin binary:

```text
src/
  lib.rs
  main.rs
  calendar/
    mod.rs
    foreground_window.rs
  events/
    mod.rs
    store.rs
  collectors/
    mod.rs
    windows.rs
```

`main.rs` is the composition root and runtime coordinator: it creates the store,
collectors, and TUI, queries events, invokes calendar construction, and connects
their lifecycles.

The `events` module owns the event types and their persistence through
`events::store`. Collectors are a separate module that creates events and writes
them through the store. The calendar module depends on event types, but not on
the event store, collectors, Windows APIs, or SQLite. The TUI depends on the
public calendar types, but not on events or collectors. This allows
reconstruction tests to run on macOS while the Windows collector is built and
tested on Windows.

## 8. MVP completion criteria

The MVP is complete when all four parts satisfy the following criteria.

### Collectors complete

- Windows foreground-window changes are collected on Windows 11 without administrator rights.
- Idle transitions are recorded using the 15-minute threshold.
- Lock and unlock are recorded if the chosen safe Windows interface supports them; otherwise application lifecycle and idle evidence safely bound activity.
- Git repositories, branch changes, HEAD changes, and commits are recorded.
- A failure in one collector does not stop the others.

### Events and storage complete

- Events survive application restarts.
- Collectors can append events without writing SQL themselves.
- Events can be queried reliably by time range.
- Schema creation and event round trips are tested.
- The database remains local under the user's profile.

### Calendar complete

- A complete daily calendar is deterministically rebuilt from stored events.
- Jira keys are recognized from the agreed branch formats and relevant visible titles.
- Coding, GitHub review, Teams activity, idle periods, and generic activity can be represented.
- A continuous context must last at least five minutes to become a block.
- Concurrent evidence is preserved rather than resolved through an arbitrary priority.
- Every calendar block can be traced to its supporting raw events.

### TUI complete

- Today's calendar appears when flogging starts.
- It refreshes every five minutes and on demand.
- The user can inspect a daily calendar and its overlapping evidence.
- Collector and refresh failures are visible without destroying the last successful view.
- It remains responsive during an ordinary workday.

### End-to-end acceptance

After several normal workdays, flogging produces a calendar that is useful enough to reconstruct the user's day without timers or continuous manual input. The application remains unobtrusive and does not require elevation.

## 9. Implementation order

Development should produce a working vertical slice early rather than finish every collector before displaying data:

1. Define the event model, `EventStore`, Calendar contract, and a basic TUI surface.
2. Implement the Windows collector and display a calendar reconstructed from foreground and idle events.
3. Implement the Git collector and add repository, branch, Jira-task, and commit context.
4. Dogfood the complete flow, turn incorrect reconstructions into test fixtures, and refine the deterministic rules.

The collector implementation order is therefore Windows and Git.

## 10. After the MVP

The first collector after the MVP should be Outlook Calendar if dogfooding shows that generic Teams activity does not provide enough meeting information. It can add scheduled meeting titles and time ranges while preserving the distinction between a scheduled meeting and observed attendance.

Other likely follow-up work includes calendar corrections, meeting-to-task mappings, Tempo submission, a long-running backend independent of the TUI, other user interfaces, additional collectors, configuration, and data-retention policies.
