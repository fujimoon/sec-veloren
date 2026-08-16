# Tracer (Structured Debug Logging) Specification

A general-purpose debugging aid that lets whatever the server generated (voxel coordinates, terrain read back after the fact, etc.) be **verified directly from a file**, instead of a human having to look at the game screen and relay screenshots back. Added while building [OS Dungeon](OsDungeon.md), after repeatedly needing screenshot round-trips just to check whether a corridor's doorway was actually open.

## 1. Purpose

- Let server-side processing be inspected directly from a file, without needing to look at the game client at all.
- Specifically, let cases where what `State::set_block()` *intended* to write diverges from what actually landed in the terrain (e.g. the corridor bugs found while building OS Dungeon) be checked mechanically by comparing coordinates, rather than by eyeballing a screenshot.
- Implemented as a generic JSON Lines logger with no dependency on Veloren or any specific feature. Currently only [OS Dungeon](OsDungeon.md) uses it, but it's meant to be reusable for debugging any future feature.

## 2. Usage

Call two functions from Rust code:

```rust
// At the start of a debugging session, clear out old events.
psypher_trace::clear();

// Append one event anywhere in the code. The second argument can be
// anything implementing Serialize — e.g. an object built with the
// serde_json::json! macro.
psypher_trace::log("room", serde_json::json!({
    "label": "current",
    "center": [17200, 10096, 819],
    "half": [10, 10, 6],
}));
```

Output goes to `psypher/trace/dungeon_trace.jsonl` (an absolute path anchored to this crate's own source directory — see "3. Architecture"), one JSON object per line. Each line carries a monotonically increasing `seq` (in call order) and the `kind` given at the call site (the event type, e.g. `"room"`, `"corridor"`, `"doorway"`, `"probe"`), plus whatever other fields were passed in, flattened into the same object.

No game client is needed to read it — a text editor, `jq`, or another agent/script can read it directly. [OS Dungeon](OsDungeon.md) uses this to record the coordinates of every room/corridor/doorway it generates, plus the actual terrain block kind read back by the `/osdungeon_probe` command — which is how its corridor bug and teleport-ordering bug were pinned down without any screenshot exchange.

## 3. Architecture

```
psypher-trace (standalone crate, psypher/trace/)
  ├─ src/lib.rs     … thin entry point: pub use tracer::{clear, log};
  └─ src/tracer.rs  … the implementation
       ├─ clear()  … truncates the trace file and resets the sequence counter
       └─ log()    … appends one line of JSON
```

- Depends only on `serde` and `serde_json` — it knows nothing about any Veloren-specific type.
- File I/O is serialized through a `std::sync::Mutex`, so concurrent calls from multiple threads/call sites can't corrupt the file.
- Each line's `seq` is a process-global monotonic counter (`AtomicU64`), deliberately *not* a wall-clock timestamp — so call ordering is always reproducible correctly even in execution environments where reading the real clock is unavailable or undesirable.
- A write failure (missing directory, no permission, etc.) is silently swallowed rather than propagated — a debugging aid must never break the feature it's debugging.
- Both `clear()` and `log()` re-ensure the directory/file exist on every call via `OpenOptions::create(true)`.
- The output path (`TRACE_FILE`) is `concat!(env!("CARGO_MANIFEST_DIR"), "/dungeon_trace.jsonl")` — an absolute path to this crate's own `Cargo.toml` directory, resolved once at build time. It does not depend on the calling process's runtime working directory at all.

### Fix history: the working-directory-dependent version

`TRACE_FILE` originally held a relative path, `"psypher/trace/dungeon_trace.jsonl"`. That worked fine for `cargo run` launched from the repo root, but calling it from `cargo test -p veloren-server` (whose working directory is `server/`) instead created a stray, duplicate `server/psypher/trace/dungeon_trace.jsonl`. This surfaced once OS Dungeon's own automated tests started calling into it, and was fixed by switching to the `CARGO_MANIFEST_DIR`-anchored absolute path above.

## 4. Files changed

- [`psypher/trace/Cargo.toml`](../../../trace/Cargo.toml) — crate definition; depends only on `serde`/`serde_json`
- [`psypher/trace/src/lib.rs`](../../../trace/src/lib.rs) — thin entry point, just `pub use tracer::{clear, log};`
- [`psypher/trace/src/tracer.rs`](../../../trace/src/tracer.rs) — the implementation

Consumers:

- `server/Cargo.toml` — path dependency on `psypher-trace`
- [`psypher/server/src/os_dungeon/os_dungeon.rs`](../../../server/src/os_dungeon/os_dungeon.rs) — where every event (`enter`/`exit`/`navigate`/`render_layout`/`room`/`corridor`/`doorway`/`punch_doorway`/`probe`) is logged from; see [OsDungeon.md](OsDungeon.md) for detail.

## 5. Known limitations

- Always-on debugging mechanism; there's no feature flag yet to exclude it from a distributed build (the same gap noted for its consumer, [OS Dungeon](OsDungeon.md)).
- No file-size cap or rotation — it keeps growing until a caller calls `clear()`.
- No unit tests (neither the happy path nor file-I/O-failure behavior of `clear`/`log` is covered by any test).
- The output location (`psypher/trace/dungeon_trace.jsonl`) is fixed at build time as an absolute path (see the fix history above), so this ever-changing debug output still lands inside the source tree. It's excluded from git (`.gitignore`) already, but moving it outside the repo entirely (e.g. a temp directory) is worth considering.

## 6. Security posture

- A purely server-internal debugging mechanism; unreachable from any client.
- Depending on what a caller (currently only [OS Dungeon](OsDungeon.md)) chooses to log, sensitive information (e.g. the server's filesystem structure) can end up in the log file. The file itself is protected only by OS-level file permissions, independent of any in-game permission system — so a real deployment should be able to disable logging output entirely (see also OsDungeon.md's "Security posture").
