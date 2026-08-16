//! Append-only JSON-lines file logger.
//!
//! Every event is one line: `{"seq": ..., "kind": "...", <your fields>}`.
//! `seq` is a monotonically increasing counter (not a wall-clock timestamp —
//! the server process may run under test harnesses that disallow real
//! clocks), so events from one process are still totally ordered relative to
//! each other even though the file itself carries no notion of real time.
//!
//! Best-effort throughout: a tracing failure (e.g. the file can't be created)
//! is swallowed rather than propagated, so a debugging aid never breaks the
//! feature it's debugging.

use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    sync::Mutex,
};

use serde::Serialize;
use serde_json::{Value, json};

/// Where the trace file lives — an absolute path anchored to *this crate's*
/// own source directory (via `CARGO_MANIFEST_DIR`, resolved once at compile
/// time on whatever machine builds it), not a path relative to the calling
/// process's current working directory. That distinction matters: a caller
/// running under `cargo test` (cwd = that test's own crate directory, e.g.
/// `server/`) or launched from some other location entirely would otherwise
/// each recreate their own stray `psypher/trace/` next to wherever they
/// happened to run from, rather than all writing to this one real file. This
/// is a dev-only debugging aid, not shipped player data, so it deliberately
/// doesn't go under `userdata/` either.
const TRACE_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dungeon_trace.jsonl");

static SEQ: AtomicU64 = AtomicU64::new(0);
static LOCK: Mutex<()> = Mutex::new(());

/// Truncate the trace file. Call this once at the start of a fresh
/// debugging session so old events don't get mixed in with new ones.
pub fn clear() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::create_dir_all(Path::new(TRACE_FILE).parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(TRACE_FILE, "");
    SEQ.store(0, Ordering::SeqCst);
}

/// Append one event. `kind` names the event type (e.g. `"enter"`, `"room"`,
/// `"corridor"`, `"probe"`); `fields` is anything `Serialize` — typically a
/// small `#[derive(Serialize)]` struct or a `serde_json::json!({...})` — and
/// gets flattened into the same JSON object as `kind`/`seq`.
pub fn log(kind: &str, fields: impl Serialize) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::create_dir_all(Path::new(TRACE_FILE).parent().unwrap_or(Path::new(".")));
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(TRACE_FILE) else {
        return;
    };

    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let mut value = json!({ "seq": seq, "kind": kind });
    if let (Value::Object(map), Ok(Value::Object(extra))) =
        (&mut value, serde_json::to_value(&fields))
    {
        map.extend(extra);
    }
    let _ = writeln!(file, "{value}");
}
