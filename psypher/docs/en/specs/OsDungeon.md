# OS Dungeon Specification

Renders the directory hierarchy of the server's filesystem as an **actual walkable 3D voxel space** inside Veloren's own game world. This extends the existing ASCII `psy dungeon` (a 2D terminal app, see [Setup.md](../Setup.md)) into the real 3D game.

## 1. Purpose

Let a directory tree be browsed intuitively as dungeon rooms and doorways.

- Intended for development/debugging use. Not intended to be run for general players in a distributed deployment (see "7. Security posture").
- Four kinds of room are ever drawn: the current directory, its parent, its siblings (up to 2 either side of the current one), and its children (up to the first 5). The whole tree is never drawn at once — only a *window* centered on wherever the player currently is.

## 2. Usage

1. Enter a world (singleplayer or multiplayer) and open chat (`Enter`).
2. Type `/osdungeon` and press Enter — enters the dungeon rooted at the server process's own working directory.
   - `/osdungeon <path>` roots it at a given directory instead (paths containing spaces work fine).
   - On entry, the player is teleported straight up 500 blocks from wherever they're currently standing, and becomes invulnerable (`Invulnerability` buff) for the duration of the visit.
3. Walk around inside the rooms to inspect the layout. Each room's walls have a doorway (an opening) punched through toward each neighboring directory's direction.
4. Typing `/osdungeon` again exits: teleports back to the pre-entry position and clears the invulnerability buff.

A debug-only `/osdungeon_probe <dx> <dy> <dz>` command is also available: it reads back the actual terrain block kind at the given offset from the active session's anchor, both replying in chat and appending to `psypher/trace/dungeon_trace.jsonl` (see below).

## 3. Architecture

```
psypher-os-dungeon-game (standalone crate, psypher/os_dungeon/game/)
  └─ siblings_window() / children_window() / list_children()
       … knows nothing about Veloren; pure functions depending only on std::fs.
         Handles hidden-dir exclusion, sorting, and skipping unreadable entries.

veloren-server (server/)
  └─ /osdungeon, /osdungeon_probe chat commands (server/src/cmd.rs)
       └─ os_dungeon::{enter, exit, navigate, probe} (physically relocated, see below)
            ├─ State::position_mut()         … teleport (same as /goto — a plain
            │                                    assignment, no "snap to free space")
            ├─ comp::Buffs (Invulnerability) … keeps the visit consequence-free
            └─ draw_room() / punch_doorway() … writes voxels directly via State::set_block()

psypher-trace (standalone crate, psypher/trace/)
  └─ Logs the coordinates of every room/corridor/doorway generated, plus whatever
     /osdungeon_probe reads back from real terrain, as JSON Lines to
     psypher/trace/dungeon_trace.jsonl — lets generated geometry be verified
     without needing screenshots.
```

### Directory layout

```
psypher/
  os_dungeon/
    game/   … shared logic (crate: psypher-os-dungeon-game). Used by both psy (ASCII) and the 3D version
    cli/    … the ASCII psy dungeon browser (crate name / command name unchanged: psy)
  server/
    src/os_dungeon/os_dungeon.rs
      … the 3D generation logic itself. Compiles as part of the `server` crate
        (referenced via a `#[path]` attribute — it depends on `Server`/`StateExt`,
         so unlike psypher-terminal it can't be a fully Veloren-independent crate)
  trace/  … the debug tracer (a general-purpose tool, not specific to OS Dungeon)
```

### Room layout (viewed from above, not to scale)

```
                [peer -2]
                [peer -1]
[parent, up]        [CURRENT]        [child 0, down]
                [peer +1]              [child 1, down]
                [peer +2]              [child 2, down]
```

The parent room sits slightly higher, and child rooms slightly lower, than the current room — a 3D version of the ASCII browser's `<` (ascend) / `>` (descend) staircase markers. It has no other relationship to actual directory depth.

Each room is a hollow 21×21×13-block box. A 4-wide, 4-tall doorway is punched through the wall facing each neighboring room's direction. Beyond the doorway is a real walkable corridor (side walls + ceiling), a boarding-ramp/airstair-style staircase where there's a height difference (parent/child). A glowing marker block sits above each doorway so it stays visible in the dark.

### Connecting siblings

Siblings sit along the same axis (north/south) their own connecting corridor would travel, so linking a farther sibling (±2) straight back to the current room would cut right through the nearer sibling (±1) sitting in between. To avoid that, siblings are chained instead: current room → nearest sibling → next sibling. (Children sit on a different, east/west axis relative to the current room, so this doesn't arise for them — each child connects to the current room's wall independently.)

## 4. Generation logic details

The generation logic itself lives in [`psypher/server/src/os_dungeon/os_dungeon.rs`](../../../server/src/os_dungeon/os_dungeon.rs).

### Constants

| Constant | Value | Meaning |
|---|---|---|
| `ROOM_HALF` | `(10, 10, 6)` | Distance from a room's center to its walls; the actual room is 21×21×13 blocks |
| `CORRIDOR_LEN` | `32` | Horizontal run of a corridor. Kept comfortably larger than `LEVEL_SHIFT` (2:1) so the resulting staircase is walkable at a normal pace |
| `ROW_SPACING` | `30` | Spacing between adjacent peer/child rooms; must exceed `2 * ROOM_HALF.y` so neighboring rooms' shells don't overlap |
| `LEVEL_SHIFT` | `16` | How much higher the parent room sits, and how much lower child rooms sit |
| `ANCHOR_UP` | `500` | How far straight up from the player the dungeon is anchored on entry |
| `WINDOW_RADIUS` | `2` | How many siblings/children either side of "current"/"selected" to draw |
| `CORRIDOR_WIDTH` | `4` | Width of a corridor/doorway |
| `DOOR_Z` | `-5` (`-ROOM_HALF.z + 1`) | Vertical start of a doorway, one block above the room's floor |
| `CORRIDOR_HEADROOM` | `4` | Interior headroom of a corridor (blocks of air above its floor) |
| `DOOR_HEIGHT` | `4` (`= CORRIDOR_HEADROOM`) | Doorway height. Must match the corridor's headroom exactly — a mismatch leaves the extra doorway height as a hole straight through to open sky above the corridor's own ceiling |
| `DRAW_CORRIDORS` | `true` | Toggle for corridor generation (currently on) |

### Drawing a room (`draw_room`)

Given a center and half-extents, scans that box once: the boundary (shell) becomes wall blocks, the interior becomes air. Rooms are always drawn as sealed boxes first, with doorways punched through afterward.

### Punching a doorway (`punch_doorway`)

A single, self-contained carve, independent of the corridor itself (`carve_corridor`) — it opens a rectangular hole of the given width/height through one wall face (`axis`: 0 = the two X-facing walls, 1 = the two Y-facing walls; `sign`: which of the pair), with a glowing marker block placed above it. The `along_offset` parameter shifts the opening away from the room's own center along the other horizontal axis — needed e.g. for the current room's east wall, which needs one doorway per child, each on that child's own row. Since it involves no multi-step interpolation, it's immune to the bug described next.

### Carving a corridor (`carve_corridor`)

Parent/child rooms sit higher/lower than the current room, so their corridor is a ramp: horizontal distance (along X or Y — never both at once) is broken into 1-block steps, and the floor height at each step is linearly interpolated between the two ends. The cross-section (one floor layer + `CORRIDOR_HEADROOM` layers of interior air + two side walls + one ceiling layer) is stamped **with no thickness along the direction of travel at all** — only the perpendicular cross-section — which is what avoids the bug below.

Two bugs turned up during development, in order of how much they mattered:

1. **End-cap bug**: an earlier version stamped each cross-section with `half + 1` blocks of thickness along the travel direction too (effectively a cube, not a slice). Right at the doorway (the very first step), that thickness could land on and overwrite the air `punch_doorway` had just opened — so the doorway visually existed but the real terrain right behind it was resealed (confirmed by reading back the actual terrain via `psypher-trace` + `/osdungeon_probe`). Stamping a travel-direction-thin slice per step, as the current implementation does, removes this "cap" entirely, so there's nothing left to reseal anything.
2. **Air priority**: adjacent steps' footprints still overlap (especially on a ramp), so a later step's "floor" could otherwise land on air a different step had already opened. To prevent that, each step's cells are accumulated into a local map rather than written to the world immediately, then flushed all at once: an "air" claim always unconditionally overwrites whatever any other step claimed for that cell, while a "floor"/"wall" claim only fills a cell that's still unclaimed — guaranteeing air always wins for a given cell regardless of the order steps are processed in.

### Placing siblings and children

- **Siblings (peers)**: `psypher-os-dungeon-game`'s `siblings_window()` returns up to `WINDOW_RADIUS` (2) entries either side of the current directory's position among its own siblings. Placed north/south of the current room, `ROW_SPACING` apart. As noted above, a farther sibling chains through the nearer one rather than connecting straight back to the current room.
- **Children**: `children_window()` returns up to `WINDOW_RADIUS * 2 + 1` entries starting from index 0 (selection always starts at the beginning). Placed east/west of the current room; each child has its own dedicated lane, so — unlike siblings — no chaining is needed.

### Session and teleport ordering

`enter()` proceeds in this order:

1. Resolve the path (the server process's working directory if no argument given).
2. Save the current position (where to return to on exit).
3. Compute the anchor (`ANCHOR_UP` blocks straight up from the current position).
4. **Draw the room/doorways before teleporting the player.**
5. Apply the `Invulnerability` buff.
6. Save the session (`OsDungeonSession`).

Step 4's ordering matters: terrain writes (`State::set_block`) aren't applied instantaneously — they're queued and applied later. Teleporting first, before the layout is actually drawn (or while stale geometry from an earlier visit is still there), could land the player inside a solid block.

The teleport itself uses `position_mut()` — the same plain coordinate assignment `/goto` uses. The more elaborate `position_mut_reposition()` (which auto-corrects to the nearest safe spot) was tried too, but that correction pass raced the terrain writes above and could land the player somewhere unintended (e.g. on top of the safety net well outside the layout) — so the plain assignment was kept instead.

### Safety net and clearing

- `clear_layout()`: blanks the full possible extent of the layout back to air on every entry/navigation, before redrawing.
- `draw_safety_net()`: lays down one floor layer well below the entire layout — insurance against a player falling off a room's outer edge and dropping hundreds of blocks to real terrain.

### `TriggerZone` (future walk-triggered-navigation data, not yet consumed)

`render_layout()` returns a `TriggerZone` list — the position/size of every doorway plus the directory it leads to. Nothing reads this yet (see "6. Known limitations"), but it's already shaped for a future ECS trigger-volume system to consume directly: "if the player enters this region, switch to this path."

### Verified with automated tests

Whether generated blocks actually land in real terrain is checked by an automated test suite (`#[cfg(test)] mod tests` inside `os_dungeon.rs`) rather than by a human looking at screenshots. Following the same pattern as the existing tests under `common/systems/tests/`, it builds a bare `common_state::State` with no server/network involved (pre-loading a grid of air chunks), calls `render_layout()`, flushes the queued `BlockChange` into real terrain via `State::tick(..., true, ...)`, and reads it back with `State::get_block()`.

- Both the current-room side and the far-room side of a parent/child doorway are actually `Air`.
- The corridor is walkable partway along its ramp (following its own interpolated path).
- A wall spot away from any doorway stays `Rock` (guards against a false pass from a broken `clear_layout` that never redraws anything).
- The spawn height lands within a doorway's vertical band.

Run with `cargo test -p veloren-server --lib os_dungeon`. Both bugs above were found and fixed by first confirming them against this test, then verifying in the running game.

## 5. Files changed

`psypher-os-dungeon-game` (standalone crate, no Veloren dependency):

- [`psypher/os_dungeon/game/Cargo.toml`](../../../os_dungeon/game/Cargo.toml)
- [`psypher/os_dungeon/game/src/lib.rs`](../../../os_dungeon/game/src/lib.rs) — `siblings_window`/`children_window`/`list_children`, plus unit tests

`psy` (the existing ASCII CLI, relocated from `psypher/psy/` to `psypher/os_dungeon/cli/`):

- [`psypher/os_dungeon/cli/src/dungeon.rs`](../../../os_dungeon/cli/src/dungeon.rs) — its own `scan()` replaced with a call into `psypher-os-dungeon-game::list_children()` (no change in look or behavior)

`psypher-trace` (new crate, general-purpose debug tracer):

- [`psypher/trace/Cargo.toml`](../../../trace/Cargo.toml)
- [`psypher/trace/src/lib.rs`](../../../trace/src/lib.rs) / [`tracer.rs`](../../../trace/src/tracer.rs)

`veloren-server` (existing crate):

- [`psypher/server/src/os_dungeon/os_dungeon.rs`](../../../server/src/os_dungeon/os_dungeon.rs) — the generation logic itself (new file, physically relocated from `server/src/os_dungeon.rs`)
- `server/src/lib.rs` — wires the module in via a `#[path]` attribute, registers the `OsDungeonSessions` resource
- `server/src/cmd.rs` — `handle_os_dungeon` / `handle_os_dungeon_probe`
- `server/Cargo.toml` — path dependencies on `psypher-os-dungeon-game` and `psypher-trace`
- `common/src/cmd.rs` — `ServerChatCommand::{OsDungeon, OsDungeonProbe}`
- `assets/voxygen/i18n/en/command.ftl` — command descriptions

## 6. Known limitations (v1)

- **Walking through a doorway does not yet navigate automatically.** Rooms are genuinely connected by walkable corridors now, but the only way to move between directories right now is re-running `/osdungeon <path>` (which exits, then re-enters rooted at the given path). The ECS trigger-volume system that would detect a doorway crossing and regenerate automatically hasn't been built yet (see `TriggerZone` above).
- No "Return" button in the HUD yet — exiting is only via re-running `/osdungeon`.
- `OsDungeonSessions` is a server-only resource, not replicated to the client. A future HUD button is expected to track its own local "am I in the dungeon" flag instead (with the caveat that it can desync if the server silently rejects entry).
- `psypher-trace` is an always-on debug mechanism; there's no feature flag yet to exclude it from a distributed build.

## 7. Security posture

- Both `/osdungeon` and `/osdungeon_probe` require the `Moderator` role. Singleplayer automatically grants Admin, so it's usable by anyone there.
- All generation is server-authoritative (the server never trusts a client-supplied position or path).
- **This feature renders the directory structure and names visible to the server process's own filesystem directly into the game world** — because of that, it is not intended to be opened up to general players on a real running server (same posture as the Terminal feature: assumes a developer running the game locally).
- The generated space floats 500 blocks above wherever it was entered from, and never overwrites existing terrain or player builds.
