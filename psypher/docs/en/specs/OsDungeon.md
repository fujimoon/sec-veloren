# OS Dungeon Specification

Renders the directory hierarchy of the server's filesystem as an **actual walkable 3D voxel space** inside Veloren's own game world. This extends the existing ASCII `psy dungeon` (a 2D terminal app, see [Setup.md](../Setup.md)) into the real 3D game.

## 1. Purpose

Let a directory tree be browsed intuitively as dungeon rooms and doorways.

- Intended for development/debugging use. Not intended to be run for general players in a distributed deployment (see "6. Security posture").
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

Each room is a hollow 21×21×13-block box. A 4-wide, 6-tall doorway is punched through the wall facing each neighboring room's direction, with a glowing marker block placed above it so it stays visible in the dark.

### Connecting siblings

Siblings sit along the same axis (north/south) their own connecting corridor would travel, so linking a farther sibling (±2) straight back to the current room would cut right through the nearer sibling (±1) sitting in between. To avoid that, siblings are chained instead: current room → nearest sibling → next sibling. (Children sit on a different, east/west axis relative to the current room, so this doesn't arise for them — each child connects to the current room's wall independently.)

## 4. Files changed

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

## 5. Known limitations (v1)

- **Corridors (tunnels) connecting rooms are currently disabled.** Each room's wall has a doorway (an opening) punched toward each neighbor, but beyond that opening is just open sky — no walkable tunnel bridges the gap between rooms yet. A bug where a ramped (height-changing) tunnel's own floor resealed its doorway was fixed, but corridor generation itself was then switched off (via the `DRAW_CORRIDORS` constant) to prioritize getting room size/doorways right first.
- **Walking through a doorway does not yet navigate automatically.** The only way to move between directories right now is re-running `/osdungeon <path>` (which exits, then re-enters rooted at the given path). The ECS trigger-volume system that would detect a doorway crossing and regenerate automatically hasn't been built yet.
- No "Return" button in the HUD yet — exiting is only via re-running `/osdungeon`.
- `OsDungeonSessions` is a server-only resource, not replicated to the client. A future HUD button is expected to track its own local "am I in the dungeon" flag instead (with the caveat that it can desync if the server silently rejects entry).
- `psypher-trace` is an always-on debug mechanism; there's no feature flag yet to exclude it from a distributed build.

## 6. Security posture

- Both `/osdungeon` and `/osdungeon_probe` require the `Moderator` role. Singleplayer automatically grants Admin, so it's usable by anyone there.
- All generation is server-authoritative (the server never trusts a client-supplied position or path).
- **This feature renders the directory structure and names visible to the server process's own filesystem directly into the game world** — because of that, it is not intended to be opened up to general players on a real running server (same posture as the Terminal feature: assumes a developer running the game locally).
- The generated space floats 500 blocks above wherever it was entered from, and never overwrites existing terrain or player builds.
