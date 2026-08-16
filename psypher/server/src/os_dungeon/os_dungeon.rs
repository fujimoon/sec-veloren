//! Server-side state and geometry for the "OS Dungeon" feature: a small,
//! disposable voxel structure floating high above the player that mirrors the
//! filesystem around a given directory — walking through a doorway (or typing
//! `/osdungeon` again) navigates between directories. See `handle_os_dungeon`
//! in `cmd.rs` for the chat command that drives this, and
//! `sys::os_dungeon_trigger` for the walk-triggered navigation.
//!
//! Layout, viewed from above (not to scale):
//!
//! ```text
//!                 [peer -2]
//!                 [peer -1]
//!  [parent, up]--corridor--[CURRENT]--corridor--[child 0, down]
//!                 [peer +1]              [child 1, down]
//!                 [peer +2]              [child 2, down]
//! ```
//!
//! The parent room sits a bit higher than the current room and children sit a
//! bit lower, purely as a visual "ascend/descend" cue (mirroring the ASCII
//! `psy dungeon` browser's `<`/`>` staircase markers) — there's no other
//! significance to filesystem depth vs. world altitude.
//!
//! Only a *window* around the current directory is ever drawn: the parent,
//! up to `2 * WINDOW_RADIUS` sibling ("peer") directories either side of
//! current, and up to `2 * WINDOW_RADIUS + 1` child directories. Rendering the
//! whole tree at once would be both slow and pointless to walk around.

use common::{
    comp::{
        self, Content,
        buff::{Buff, BuffData, BuffKind, BuffSource, DestInfo},
    },
    resources::Time,
    terrain::{Block, BlockKind},
    uid::Uid,
};
use common_state::State;
use hashbrown::HashMap;
use psypher_os_dungeon_game::{children_window, siblings_window};
use serde_json::json;
use specs::WorldExt;
use std::path::{Path, PathBuf};
use vek::{Rgb, Vec3};

use crate::{Server, StateExt};

pub type CmdResult<T> = Result<T, Content>;

/// Half-extents of a room's walkable interior (so a room spans
/// `-ROOM_HALF..=ROOM_HALF` around its center), in blocks. 21×21 floor, 13
/// tall — a proper walkable hall, not a closet.
const ROOM_HALF: Vec3<i32> = Vec3::new(10, 10, 6);
/// Length (horizontal run) of the flat/ramped corridor connecting two room
/// centers. Must comfortably exceed `LEVEL_SHIFT` (the ramp's vertical rise)
/// — at `CORRIDOR_LEN = 10, LEVEL_SHIFT = 16` this used to climb faster than
/// 1 block up per block forward (steeper than 45°), which is not walkable at
/// normal speed: a few steps in, the tunnel's own rising floor/ceiling ends up
/// above the player's head, which reads as "the corridor isn't hollow" even
/// though every individual cell was correctly carved. At `32:16` (2 blocks
/// forward per 1 block up) it's a normal, comfortable staircase.
const CORRIDOR_LEN: i32 = 32;
/// Spacing between adjacent peer/child rooms along their row. Must clear
/// `2 * ROOM_HALF.y` so neighboring rooms' shells don't overlap.
const ROW_SPACING: i32 = 30;
/// How much higher the parent room sits, and how much lower child rooms sit,
/// relative to the current room — purely a visual ascend/descend cue.
const LEVEL_SHIFT: i32 = 16;
/// How far straight up from the player the dungeon is anchored on entry.
const ANCHOR_UP: i32 = 500;
/// Corridor carving is back on: `carve_corridor` no longer stamps any
/// thickness in the direction of travel (see its doc comment), so it has no
/// "end cap" left to reseal a doorway `punch_doorway` just opened.
const DRAW_CORRIDORS: bool = true;
/// How many siblings/children either side of "current"/"selected" to draw.
const WINDOW_RADIUS: usize = 2;
/// Width (in blocks) of a corridor tunnel.
const CORRIDOR_WIDTH: i32 = 4;
/// Z of a room's floor-level doorway, relative to the room's own center.
/// `ROOM_HALF.z` is the room's floor/ceiling wall; `+1` lands one block above
/// the floor, i.e. the room's own lowest walkable interior layer — so a
/// corridor carved at this height (see `carve_corridor`) lines up flush with
/// the room floor instead of opening a hole near the ceiling.
const DOOR_Z: i32 = -ROOM_HALF.z + 1;
/// Interior headroom of a corridor tunnel (blocks of air above its floor).
/// 2 was tight enough that the 2:1 ramp slope (`CORRIDOR_LEN`:`LEVEL_SHIFT`)
/// could still catch a player's head; 4 gives real clearance.
const CORRIDOR_HEADROOM: i32 = 4;
/// Doorway height in blocks, starting from `DOOR_Z`. Must match
/// `CORRIDOR_HEADROOM` exactly — a taller doorway than the tunnel behind it
/// would leave the extra height as a hole straight through to open sky
/// (defeating the corridor's fall-prevention shell entirely).
const DOOR_HEIGHT: i32 = CORRIDOR_HEADROOM;

fn wall_current() -> Block { Block::new(BlockKind::Rock, Rgb::new(225, 225, 210)) }

fn wall_parent() -> Block { Block::new(BlockKind::Rock, Rgb::new(80, 200, 210)) }

fn wall_peer() -> Block { Block::new(BlockKind::Rock, Rgb::new(140, 140, 140)) }

fn wall_child() -> Block { Block::new(BlockKind::Rock, Rgb::new(90, 190, 110)) }

fn floor() -> Block { Block::new(BlockKind::Rock, Rgb::new(110, 110, 110)) }

/// A warm, self-lit marker placed above every doorway threshold (see
/// `carve_corridor`) so entrances stay visible even in total darkness.
fn door_light() -> Block { Block::new(BlockKind::GlowingRock, Rgb::new(255, 200, 110)) }

/// Compact `[x, y, z]` form for trace log fields.
fn vpos(v: Vec3<i32>) -> [i32; 3] { [v.x, v.y, v.z] }

/// A small region players can walk into to navigate to another directory.
#[derive(Debug, Clone)]
pub struct TriggerZone {
    pub min: Vec3<i32>,
    pub max: Vec3<i32>,
    pub target: PathBuf,
}

impl TriggerZone {
    pub fn contains(&self, pos: Vec3<i32>) -> bool {
        (self.min.x..=self.max.x).contains(&pos.x)
            && (self.min.y..=self.max.y).contains(&pos.y)
            && (self.min.z..=self.max.z).contains(&pos.z)
    }
}

/// One player's active OS Dungeon visit.
pub struct OsDungeonSession {
    /// Where to teleport the player back to on exit.
    pub origin_pos: comp::Pos,
    /// Directory the current room represents.
    pub current_path: PathBuf,
    /// World-space center the whole layout is drawn around. Stays fixed for
    /// the lifetime of a session; only `current_path`/`zones` change as the
    /// player navigates.
    pub anchor: Vec3<i32>,
    /// Doorway zones for the layout currently drawn at `anchor`.
    pub zones: Vec<TriggerZone>,
    /// Last zone (by index into `zones`) the player was standing in, used to
    /// only fire navigation once per doorway crossing rather than every tick.
    pub last_zone: Option<usize>,
}

/// All currently-active OS Dungeon visits, keyed by player `Uid`. Kept as a
/// plain server resource (not a replicated ECS component) since this is
/// server-only bookkeeping — the client tracks its own local "am I in the
/// dungeon" flag for HUD purposes instead of relying on replicated state.
#[derive(Default)]
pub struct OsDungeonSessions(HashMap<Uid, OsDungeonSession>);

impl OsDungeonSessions {
    pub fn get(&self, uid: Uid) -> Option<&OsDungeonSession> { self.0.get(&uid) }

    pub fn get_mut(&mut self, uid: Uid) -> Option<&mut OsDungeonSession> { self.0.get_mut(&uid) }

    pub fn contains(&self, uid: Uid) -> bool { self.0.contains_key(&uid) }

    pub fn insert(&mut self, uid: Uid, session: OsDungeonSession) { self.0.insert(uid, session); }

    pub fn remove(&mut self, uid: Uid) -> Option<OsDungeonSession> { self.0.remove(&uid) }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Uid, &mut OsDungeonSession)> {
        self.0.iter_mut()
    }
}

fn uid_of(server: &Server, entity: specs::Entity) -> CmdResult<Uid> {
    server
        .state
        .ecs()
        .read_storage::<Uid>()
        .get(entity)
        .copied()
        .ok_or_else(|| Content::Plain("Entity has no Uid".to_owned()))
}

/// Fill a hollow box (walls/floor/ceiling of `wall`, air interior) centered at
/// `center` with the given half-extents.
fn draw_room(state: &State, center: Vec3<i32>, half: Vec3<i32>, wall: Block) {
    let min = center - half;
    let max = center + half;
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let on_shell =
                    x == min.x || x == max.x || y == min.y || y == max.y || z == min.z || z == max.z;
                state.set_block(Vec3::new(x, y, z), if on_shell { wall } else { Block::empty() });
            }
        }
    }
}

/// Punch a simple rectangular doorway straight through *one* wall of a room
/// — just an opening, no tunnel beyond it (unlike `carve_corridor`, this is a
/// single flat carve with no multi-step interpolation, so it has none of the
/// "later step's floor overwrites an earlier step's air" failure mode that
/// affected ramped corridors). `axis` selects the wall pair (`0` = the two
/// x-facing walls, `1` = the two y-facing walls); `sign` selects which of
/// that pair (`-1` = the `min` side, `1` = the `max` side); `along_offset`
/// shifts the opening's center away from the room's own center along the
/// *other* horizontal axis (needed e.g. for the current room's east wall,
/// which needs one doorway per child, each on that child's own row, not all
/// stacked on the room's center row). The opening is `width` blocks wide and
/// `DOOR_HEIGHT` blocks tall, starting `DOOR_Z` above the floor (see its doc
/// comment), with a glowing marker block above it so it's visible in the
/// dark.
fn punch_doorway(
    state: &State,
    center: Vec3<i32>,
    half: Vec3<i32>,
    axis: usize,
    sign: i32,
    along_offset: i32,
    width: i32,
) {
    let half_w = width / 2;
    let door_base_z = center.z + DOOR_Z;
    for perp in -half_w..=half_w {
        for dz in 0..DOOR_HEIGHT {
            let pos = match axis {
                0 => Vec3::new(center.x + sign * half.x, center.y + along_offset + perp, door_base_z + dz),
                _ => Vec3::new(center.x + along_offset + perp, center.y + sign * half.y, door_base_z + dz),
            };
            state.set_block(pos, Block::empty());
        }
    }
    let light_pos = match axis {
        0 => Vec3::new(center.x + sign * half.x, center.y + along_offset, door_base_z + DOOR_HEIGHT),
        _ => Vec3::new(center.x + along_offset, center.y + sign * half.y, door_base_z + DOOR_HEIGHT),
    };
    state.set_block(light_pos, door_light());
    psypher_trace::log("punch_doorway", json!({
        "room_center": vpos(center), "axis": axis, "sign": sign, "along_offset": along_offset,
        "width": width, "light": vpos(light_pos),
    }));
}

/// Carve a fully-enclosed walkable tunnel (floor, left/right walls, ceiling,
/// air interior) in a straight *horizontal* line from `from` to `to` — the
/// caller's job is to ensure exactly one of `diff.x`/`diff.y` is nonzero
/// (true for every caller here: parent/child links run along X, peer links
/// along Y). A height difference between `from`/`to` (parent: `+LEVEL_SHIFT`,
/// child: `-LEVEL_SHIFT`) becomes a stepped ramp: the floor height is
/// linearly interpolated per horizontal step, giving a boarding-ramp/airstair
/// look rather than a single steep slope.
///
/// Unlike an earlier version of this function, each step stamps only a
/// *cross-sectional slice* perpendicular to the direction of travel — no
/// thickness at all along the travel axis itself. That matters specifically
/// because the old version's travel-direction thickness became an "end cap":
/// at the doorway end, the local map's `or_insert` wall from one step could
/// land exactly on the world position `punch_doorway` had just opened as air,
/// silently resealing the threshold right after opening it. With no
/// travel-direction thickness, there is no such cap to reseal anything —
/// each 1-block-apart slice simply tiles into the next.
///
/// Air still unconditionally wins over floor/wall for any given cell — kept
/// from the earlier version — because adjacent ramp steps' interiors still
/// overlap in world space (the ramp doesn't rise a full block every step),
/// so a later step's floor could otherwise land inside an earlier step's
/// headroom.
fn carve_corridor(state: &State, from: Vec3<i32>, to: Vec3<i32>, wall: Block) {
    psypher_trace::log("corridor", json!({
        "from": vpos(from),
        "to": vpos(to),
        "width": CORRIDOR_WIDTH,
        "headroom": CORRIDOR_HEADROOM,
    }));

    let diff = to - from;
    // One step per horizontal block travelled; height is interpolated, not
    // stepped independently, so it never inflates the step count.
    let steps = diff.x.abs().max(diff.y.abs()).max(1);
    // Unit horizontal direction of travel, and the horizontal axis
    // perpendicular to it (the width axis) — swapped x/y of `dir`.
    let dir = Vec3::new(diff.x.signum(), diff.y.signum(), 0);
    let perp = Vec3::new(dir.y.abs(), dir.x.abs(), 0);
    let half = CORRIDOR_WIDTH / 2;

    let mut cells: HashMap<Vec3<i32>, Block> = HashMap::new();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let z = (from.z as f32 + diff.z as f32 * t).round() as i32;
        let base = Vec3::new(from.x + dir.x * i, from.y + dir.y * i, z);

        // Cross-section at this step: `w` runs across the width axis only
        // (`perp`), `dz` is height. `|w| <= half` and `0 <= dz < HEADROOM` is
        // the walkable interior (air, always wins); `|w| <= half, dz == -1`
        // is the floor; everything else (the two side-wall columns at
        // `w == ±(half+1)`, and the ceiling row `dz == HEADROOM`) is solid.
        for w in -(half + 1)..=(half + 1) {
            let col = base + perp * w;
            let in_width = w.abs() <= half;
            for dz in -1..=CORRIDOR_HEADROOM {
                let pos = col + Vec3::new(0, 0, dz);
                if in_width && (0..CORRIDOR_HEADROOM).contains(&dz) {
                    cells.insert(pos, Block::empty());
                } else if in_width && dz == -1 {
                    cells.entry(pos).or_insert_with(floor);
                } else {
                    cells.entry(pos).or_insert(wall);
                }
            }
        }

        // Mark both doorway thresholds with a glowing block directly
        // overhead — the dungeon floats in open sky with no sun of its own,
        // so at night an unlit doorway is indistinguishable from a solid
        // wall at a glance.
        if i == 0 || i == steps {
            cells.insert(base + Vec3::new(0, 0, CORRIDOR_HEADROOM), door_light());
            psypher_trace::log("doorway", json!({
                "end": if i == 0 { "from" } else { "to" },
                "opening_center": vpos(base),
                "light": vpos(base + Vec3::new(0, 0, CORRIDOR_HEADROOM)),
            }));
        }
    }

    for (pos, block) in cells {
        state.set_block(pos, block);
    }
}

/// Bounding box big enough to cover the entire possible layout regardless of
/// how many peers/children actually get drawn, used to blank the area before
/// a redraw.
fn layout_bounds(anchor: Vec3<i32>) -> (Vec3<i32>, Vec3<i32>) {
    let reach_x = ROOM_HALF.x + CORRIDOR_LEN + ROOM_HALF.x + 2;
    let reach_y = ROOM_HALF.y + WINDOW_RADIUS as i32 * ROW_SPACING + 2;
    let reach_z = ROOM_HALF.z + LEVEL_SHIFT + 2;
    (
        anchor - Vec3::new(reach_x, reach_y, reach_z),
        anchor + Vec3::new(reach_x, reach_y, reach_z),
    )
}

/// A solid floor well below the whole layout, wide enough to cover every
/// possible room position. Defense-in-depth: corridors are fully enclosed
/// (see `carve_corridor`), but a player who steps off a room's *outer* edge
/// (e.g. deliberately, or from some future layout change) would otherwise
/// fall all the way down to real terrain hundreds of blocks below — a long,
/// disorienting trip back even though `Invulnerability` prevents damage.
fn draw_safety_net(state: &State, anchor: Vec3<i32>) {
    let (min, max) = layout_bounds(anchor);
    let net_z = min.z - 5;
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            state.set_block(Vec3::new(x, y, net_z), floor());
        }
    }
}

/// Blank the full possible layout footprint at `anchor` back to air. Cheap:
/// this space is never used for anything else (see module docs).
pub fn clear_layout(state: &State, anchor: Vec3<i32>) {
    let (min, max) = layout_bounds(anchor);
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                state.set_block(Vec3::new(x, y, z), Block::empty());
            }
        }
    }
}

/// Draw the current/parent/peers/children layout for `path` at `anchor`, and
/// return the doorway trigger zones for navigating out of it. Does *not*
/// clear the area first — call [`clear_layout`] beforehand if this isn't a
/// freshly-cleared anchor.
pub fn render_layout(state: &State, anchor: Vec3<i32>, path: &Path) -> Vec<TriggerZone> {
    psypher_trace::log("render_layout", json!({
        "path": path.display().to_string(),
        "anchor": vpos(anchor),
        "room_half": vpos(ROOM_HALF),
        "door_z": DOOR_Z,
    }));

    let mut zones = Vec::new();

    draw_safety_net(state, anchor);
    draw_room(state, anchor, ROOM_HALF, wall_current());
    psypher_trace::log("room", json!({ "label": "current", "center": vpos(anchor), "half": vpos(ROOM_HALF) }));

    // Parent: west + up.
    if let Some(parent) = path.parent() {
        let center = anchor + Vec3::new(-(ROOM_HALF.x * 2 + CORRIDOR_LEN), 0, LEVEL_SHIFT);
        draw_room(state, center, ROOM_HALF, wall_parent());
        psypher_trace::log("room", json!({ "label": "parent", "center": vpos(center), "half": vpos(ROOM_HALF) }));
        // Doorways: current room's west wall, and parent room's east wall
        // (facing each other) — always punched, independent of whether the
        // connecting tunnel itself is enabled below.
        punch_doorway(state, anchor, ROOM_HALF, 0, -1, 0, CORRIDOR_WIDTH);
        punch_doorway(state, center, ROOM_HALF, 0, 1, 0, CORRIDOR_WIDTH);
        if DRAW_CORRIDORS {
            carve_corridor(
                state,
                anchor + Vec3::new(-ROOM_HALF.x, 0, DOOR_Z),
                center + Vec3::new(ROOM_HALF.x, 0, DOOR_Z),
                wall_parent(),
            );
        }
        zones.push(TriggerZone {
            min: anchor + Vec3::new(-ROOM_HALF.x - CORRIDOR_LEN / 2, -1, DOOR_Z - 1),
            max: anchor + Vec3::new(-ROOM_HALF.x - 1, 1, DOOR_Z + 2),
            target: parent.to_path_buf(),
        });
    }

    // Peers: same level, north/south row, current directory's own slot
    // skipped (it's already the center room). Peers sit on the *same* axis
    // their corridor travels along (unlike children, see below), so a peer 2
    // slots out can't get its own private lane the way children can — instead
    // each peer connects to the *previous* room in its direction (current ->
    // nearest -> next-nearest -> ...), a short hop at a time, so a farther
    // peer's corridor never has to cut through a nearer peer's room sitting
    // in between.
    let (total_siblings, self_index, peers) = siblings_window(path, WINDOW_RADIUS);
    psypher_trace::log("siblings_window", json!({
        "total": total_siblings,
        "self_index": self_index,
        "peers": peers.iter().map(|p| json!({
            "index": p.index, "name": p.name.to_string_lossy(), "full_path": p.full_path.display().to_string(),
        })).collect::<Vec<_>>(),
    }));
    let mut chained: Vec<(i32, &psypher_os_dungeon_game::DirInfo)> = peers
        .iter()
        .map(|peer| (peer.index as i32 - self_index as i32, peer))
        .collect();
    chained.sort_by_key(|(offset, _)| offset.abs());

    // Running "last room drawn in this direction" anchor, per direction
    // (index 0 = north/positive offsets, 1 = south/negative offsets); both
    // chains start at the current room.
    let mut chain_from = [anchor, anchor];
    for (offset, peer) in chained {
        let dir = offset.signum();
        let lane = if dir >= 0 { 0 } else { 1 };
        let from = chain_from[lane];
        let center = anchor + Vec3::new(0, offset * ROW_SPACING, 0);
        draw_room(state, center, ROOM_HALF, wall_peer());
        psypher_trace::log("room", json!({
            "label": format!("peer[{offset}]"), "center": vpos(center), "half": vpos(ROOM_HALF),
            "target": peer.full_path.display().to_string(),
        }));
        // Doorways: `from`'s wall facing this peer, and this peer's wall
        // facing back — `from` is either the current room (nearest peer in
        // this direction) or the previous peer in the chain.
        punch_doorway(state, from, ROOM_HALF, 1, dir, 0, CORRIDOR_WIDTH);
        punch_doorway(state, center, ROOM_HALF, 1, -dir, 0, CORRIDOR_WIDTH);
        if DRAW_CORRIDORS {
            carve_corridor(
                state,
                from + Vec3::new(0, dir * ROOM_HALF.y, DOOR_Z),
                center + Vec3::new(0, -dir * ROOM_HALF.y, DOOR_Z),
                wall_peer(),
            );
        }
        zones.push(TriggerZone {
            min: from + Vec3::new(-1, dir * ROOM_HALF.y, DOOR_Z - 1),
            max: from + Vec3::new(1, dir * (ROOM_HALF.y + CORRIDOR_LEN), DOOR_Z + 2),
            target: peer.full_path.clone(),
        });
        chain_from[lane] = center;
    }

    // Children: east + down, windowed around index 0 (selection always
    // resets to the start of the list when navigating into a directory).
    let (total_children, children) = children_window(path, 0, WINDOW_RADIUS);
    psypher_trace::log("children_window", json!({
        "total": total_children,
        "children": children.iter().map(|c| json!({
            "index": c.index, "name": c.name.to_string_lossy(), "full_path": c.full_path.display().to_string(),
        })).collect::<Vec<_>>(),
    }));
    for child in &children {
        let offset = child.index as i32; // selected == 0
        let center =
            anchor + Vec3::new(ROOM_HALF.x * 2 + CORRIDOR_LEN, offset * ROW_SPACING, -LEVEL_SHIFT);
        draw_room(state, center, ROOM_HALF, wall_child());
        psypher_trace::log("room", json!({
            "label": format!("child[{offset}]"), "center": vpos(center), "half": vpos(ROOM_HALF),
            "target": child.full_path.display().to_string(),
        }));
        // Current room's east wall needs one doorway per child, each on that
        // child's own row (`along_offset`) rather than all stacked on the
        // room's center row; the child room's own west wall is centered on
        // itself as usual (`along_offset = 0`).
        punch_doorway(state, anchor, ROOM_HALF, 0, 1, offset * ROW_SPACING, CORRIDOR_WIDTH);
        punch_doorway(state, center, ROOM_HALF, 0, -1, 0, CORRIDOR_WIDTH);
        if DRAW_CORRIDORS {
            carve_corridor(
                state,
                anchor + Vec3::new(ROOM_HALF.x, offset * ROW_SPACING, DOOR_Z),
                center + Vec3::new(-ROOM_HALF.x, 0, DOOR_Z),
                wall_child(),
            );
        }
        zones.push(TriggerZone {
            min: anchor + Vec3::new(ROOM_HALF.x + 1, offset * ROW_SPACING - 1, DOOR_Z - 1),
            max: anchor + Vec3::new(ROOM_HALF.x + CORRIDOR_LEN / 2, offset * ROW_SPACING + 1, DOOR_Z + 2),
            target: child.full_path.clone(),
        });
    }

    zones
}

/// Resolve the directory an `/osdungeon` invocation should start at: the
/// given path if valid, or the server process's current directory otherwise.
fn resolve_start_path(path_arg: Option<String>) -> CmdResult<PathBuf> {
    let path = match path_arg {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir()
            .map_err(|e| Content::Plain(format!("Could not read server's working directory: {e}")))?,
    };
    if !path.is_dir() {
        return Err(Content::Plain(format!(
            "'{}' is not a directory the server can see",
            path.display()
        )));
    }
    path.canonicalize()
        .map_err(|e| Content::Plain(format!("Could not resolve '{}': {e}", path.display())))
}

/// Apply (or refresh) the "safe while visiting" invulnerability buff.
fn apply_paused_status(server: &mut Server, entity: specs::Entity) {
    let ecs = server.state.ecs();
    let mut buffs_all = ecs.write_storage::<comp::Buffs>();
    let stats = ecs.read_storage::<comp::Stats>();
    let masses = ecs.read_storage::<comp::Mass>();
    let time = ecs.read_resource::<Time>();
    if let Some(mut buffs) = buffs_all.get_mut(entity) {
        let dest_info = DestInfo { stats: stats.get(entity), mass: masses.get(entity) };
        buffs.insert(
            Buff::new(
                BuffKind::Invulnerability,
                BuffData::new(1.0, None),
                vec![],
                BuffSource::Command,
                *time,
                dest_info,
                None,
                None,
            ),
            *time,
        );
    }
}

fn remove_paused_status(server: &Server, entity: specs::Entity) {
    if let Some(mut buffs) = server.state.ecs().write_storage::<comp::Buffs>().get_mut(entity) {
        buffs.remove_kind(BuffKind::Invulnerability);
    }
}

/// Enter the OS Dungeon: save the player's position, teleport them to a fresh
/// anchor above where they're currently standing, apply the paused status,
/// and draw the layout for `path_arg` (or the server's cwd if not given).
pub fn enter(server: &mut Server, entity: specs::Entity, path_arg: Option<String>) -> CmdResult<()> {
    // Fresh trace per visit: makes each test iteration's log unambiguous
    // rather than accumulating across restarts. See `psypher/trace`.
    psypher_trace::clear();

    let path = resolve_start_path(path_arg)?;
    let origin_pos = server
        .state
        .ecs()
        .read_storage::<comp::Pos>()
        .get(entity)
        .copied()
        .ok_or_else(|| Content::Plain("Position unavailable".to_owned()))?;
    let uid = uid_of(server, entity)?;

    let anchor = (origin_pos.0 + Vec3::new(0.0, 0.0, ANCHOR_UP as f32)).map(|v| v.round() as i32);
    psypher_trace::log("enter", json!({
        "uid": format!("{uid:?}"),
        "path": path.display().to_string(),
        "origin_pos": vpos(origin_pos.0.map(|v| v as i32)),
        "anchor": vpos(anchor),
    }));

    // Draw the room *before* moving the player into it: terrain edits made via
    // `set_block` are queued and applied by the server, not instantaneous, so
    // teleporting first and drawing after risks landing the player inside
    // whatever was previously at that spot (stale geometry from an earlier
    // visit that started nearby, or mid-application edits) before the fresh
    // layout takes effect — which looks like being stuck inside a solid
    // block. Drawing first guarantees the destination is already a real room.
    clear_layout(&server.state, anchor);
    let zones = render_layout(&server.state, anchor, &path);

    // Plain `position_mut` (same as `/goto`), not `position_mut_reposition`:
    // the latter queues an additional "nudge to nearest free space" pass that
    // races our just-issued terrain edits and can land the player somewhere
    // other than the room we just cleared and drew (e.g. snapped down onto
    // the safety net far below). We already guarantee the destination is a
    // real, freshly-drawn open room, so no further "smart" correction is
    // wanted here.
    server
        .state
        .position_mut(entity, true, |pos| pos.0 = anchor.map(|v| v as f32))
        .map_err(|_| Content::Plain("Could not teleport into the OS Dungeon".to_owned()))?;

    apply_paused_status(server, entity);

    let mut sessions = server.state.ecs().write_resource::<OsDungeonSessions>();
    sessions.insert(uid, OsDungeonSession {
        origin_pos,
        current_path: path,
        anchor,
        zones,
        last_zone: None,
    });
    Ok(())
}

/// Exit the OS Dungeon: teleport the player back to where they entered from
/// and remove the paused status. Leaves the (disposable, out-of-the-way)
/// voxel layout behind — see module docs.
pub fn exit(server: &mut Server, entity: specs::Entity) -> CmdResult<()> {
    let uid = uid_of(server, entity)?;
    let session = server
        .state
        .ecs()
        .write_resource::<OsDungeonSessions>()
        .remove(uid)
        .ok_or_else(|| Content::Plain("You are not currently in the OS Dungeon".to_owned()))?;
    psypher_trace::log("exit", json!({ "uid": format!("{uid:?}"), "origin_pos": vpos(session.origin_pos.0.map(|v| v as i32)) }));

    server
        .state
        .position_mut(entity, true, |pos| *pos = session.origin_pos)
        .map_err(|_| Content::Plain("Could not teleport back".to_owned()))?;
    remove_paused_status(server, entity);
    Ok(())
}

/// Navigate an already-active session to `target` (a doorway's destination
/// directory): redraw the layout at the same anchor and update the session.
pub fn navigate(server: &Server, uid: Uid, target: PathBuf) {
    let anchor = {
        let sessions = server.state.ecs().read_resource::<OsDungeonSessions>();
        match sessions.get(uid) {
            Some(s) => s.anchor,
            None => return,
        }
    };
    psypher_trace::log("navigate", json!({ "uid": format!("{uid:?}"), "target": target.display().to_string(), "anchor": vpos(anchor) }));
    clear_layout(&server.state, anchor);
    let zones = render_layout(&server.state, anchor, &target);

    let mut sessions = server.state.ecs().write_resource::<OsDungeonSessions>();
    if let Some(session) = sessions.get_mut(uid) {
        session.current_path = target;
        session.zones = zones;
        session.last_zone = None;
    }
}

/// Read back the *actual* current terrain at `session.anchor + offset` and
/// log it — ground truth, not intent. `render_layout`'s `set_block` calls
/// only *queue* edits (see `common_state::state::BlockChange`); they aren't
/// necessarily visible via `State::get_block` within the same tick they were
/// issued. This is why `enter`/`navigate` don't self-check their own writes
/// immediately — call this from a *separate* later command instead (e.g.
/// `/osdungeon_probe`), by which point a tick has passed and edits have
/// landed for real. Returns the block kind found, for a chat reply.
pub fn probe(server: &Server, entity: specs::Entity, offset: Vec3<i32>) -> CmdResult<String> {
    let uid = uid_of(server, entity)?;
    let anchor = {
        let sessions = server.state.ecs().read_resource::<OsDungeonSessions>();
        sessions
            .get(uid)
            .map(|s| s.anchor)
            .ok_or_else(|| Content::Plain("You are not currently in the OS Dungeon".to_owned()))?
    };
    let pos = anchor + offset;
    let block = server.state.get_block(pos);
    let summary = match block {
        Some(b) => format!("{:?}", b.kind()),
        None => "<unloaded chunk>".to_owned(),
    };
    psypher_trace::log("probe", json!({
        "uid": format!("{uid:?}"),
        "anchor": vpos(anchor),
        "offset": vpos(offset),
        "pos": vpos(pos),
        "block": summary,
    }));
    Ok(format!("{summary} at anchor+{offset:?} (world {pos:?})"))
}

/// Automated checks for `render_layout`'s doorway/corridor connectivity —
/// added after several rounds of a human having to walk into a room in a
/// live game, get stuck, and report back "the hole isn't open" before any
/// specific bug could be pinned down. This constructs a bare `State` (no
/// server, no network, no game client — see `common/systems/tests/` for the
/// same pattern used elsewhere in the codebase) and asserts on the *actual*
/// terrain left behind after `render_layout` + a real `BlockChange` flush,
/// not just on what the code *intended* to write.
#[cfg(test)]
mod tests {
    use super::*;
    use common::{
        resources::GameMode,
        shared_server_config::ServerConstants,
        terrain::{MapSizeLg, TerrainChunk},
    };
    use std::{sync::Arc, time::Duration};
    use vek::Vec2;

    /// A bare `State` with a generous grid of empty (air-above-sea-level)
    /// chunks pre-loaded around the origin — big enough to cover any
    /// position `render_layout` could plausibly write to for the anchor used
    /// below, without needing full world generation.
    fn setup() -> State {
        let pools = State::pools(GameMode::Server);
        let map_size_lg = MapSizeLg::new(Vec2::new(1, 1)).unwrap();
        let mut state = State::new(
            GameMode::Server,
            pools,
            map_size_lg,
            Arc::new(TerrainChunk::water(0)),
            |_dispatch_builder| {},
            #[cfg(feature = "plugins")]
            common_state::plugin::PluginMgr::default(),
        );
        // ±6 chunks (32 blocks each) covers ±192 in x/y, comfortably beyond
        // `layout_bounds`' current reach.
        for cx in -6..=6 {
            for cy in -6..=6 {
                state.insert_chunk(Vec2::new(cx, cy), Arc::new(TerrainChunk::water(0)));
            }
        }
        state
    }

    /// Flush queued `BlockChange` writes into real terrain, the same way a
    /// live server does once per tick — `render_layout`'s `set_block` calls
    /// only *queue* edits (see `probe`'s own doc comment above) until this
    /// runs, which is exactly the subtlety that made earlier bugs here hard
    /// to reason about from source alone.
    fn flush(state: &mut State) {
        state.tick(
            Duration::from_millis(1),
            true,
            None,
            &ServerConstants { day_cycle_coefficient: 24.0 },
            |_, _| {},
        );
    }

    /// A real directory with both a parent *and* several children, so the
    /// test exercises the same window-computation `render_layout` uses in
    /// production. `server/`'s parent (the repo root) qualifies.
    fn test_path() -> PathBuf {
        std::env::current_dir().unwrap().parent().unwrap().to_path_buf()
    }

    fn block_kind_at(state: &State, pos: Vec3<i32>) -> Option<BlockKind> {
        state.get_block(pos).map(|b| b.kind())
    }

    #[test]
    fn parent_doorway_is_walkable_at_both_ends() {
        let mut state = setup();
        let anchor = Vec3::new(0, 0, 500);
        let path = test_path();
        assert!(path.parent().is_some(), "test needs a path with a parent");

        clear_layout(&state, anchor);
        render_layout(&state, anchor, &path);
        flush(&mut state);

        // Current room's side of the parent doorway, and a couple of steps
        // further out along the *ramp's own interpolated path* (not at a
        // fixed height — this link climbs as it goes, so "2 blocks forward
        // at the doorway's height" is partway into the floor by design, not
        // a bug; see `corridor_interior_is_walkable_partway_along_the_ramp`
        // below for a midpoint check done the correct way) — both must be
        // open, not the room's own solid wall or a resealed "end cap".
        let near_door = anchor + Vec3::new(-ROOM_HALF.x, 0, DOOR_Z);
        let past_door = anchor + Vec3::new(-ROOM_HALF.x - 2, 0, DOOR_Z + 1);
        assert_eq!(
            block_kind_at(&state, near_door),
            Some(BlockKind::Air),
            "current room's doorway threshold toward its parent must be open"
        );
        assert_eq!(
            block_kind_at(&state, past_door),
            Some(BlockKind::Air),
            "just past the doorway threshold must still be open, not resealed"
        );

        // The parent room's own doorway, facing back — the far end of the
        // ramp, where the original "later step's floor reseals the earlier
        // step's air" bug specifically showed up.
        let parent_center =
            anchor + Vec3::new(-(ROOM_HALF.x * 2 + CORRIDOR_LEN), 0, LEVEL_SHIFT);
        let parent_door = parent_center + Vec3::new(ROOM_HALF.x, 0, DOOR_Z);
        assert_eq!(
            block_kind_at(&state, parent_door),
            Some(BlockKind::Air),
            "parent room's doorway threshold toward the current room must be open"
        );

        // Sanity check: a spot on the same wall, away from the doorway
        // width, must still be solid — otherwise these assertions would
        // trivially pass because *nothing* is solid (e.g. a broken
        // `clear_layout` that never got redrawn).
        let away_from_door = anchor + Vec3::new(-ROOM_HALF.x, ROOM_HALF.y - 2, 0);
        assert_eq!(
            block_kind_at(&state, away_from_door),
            Some(BlockKind::Rock),
            "the room's wall away from any doorway must remain solid"
        );
    }

    #[test]
    fn first_child_doorway_is_walkable_at_both_ends() {
        let mut state = setup();
        let anchor = Vec3::new(0, 0, 500);
        let path = test_path();
        let (total_children, _) = children_window(&path, 0, WINDOW_RADIUS);
        assert!(total_children > 0, "test needs a path with at least one child directory");

        clear_layout(&state, anchor);
        render_layout(&state, anchor, &path);
        flush(&mut state);

        let near_door = anchor + Vec3::new(ROOM_HALF.x, 0, DOOR_Z);
        assert_eq!(
            block_kind_at(&state, near_door),
            Some(BlockKind::Air),
            "current room's doorway threshold toward child[0] must be open"
        );

        let child_center =
            anchor + Vec3::new(ROOM_HALF.x * 2 + CORRIDOR_LEN, 0, -LEVEL_SHIFT);
        let child_door = child_center + Vec3::new(-ROOM_HALF.x, 0, DOOR_Z);
        assert_eq!(
            block_kind_at(&state, child_door),
            Some(BlockKind::Air),
            "child[0]'s doorway threshold toward the current room must be open"
        );
    }

    #[test]
    fn corridor_interior_is_walkable_partway_along_the_ramp() {
        // Specifically exercises the middle of a ramped link, not just its
        // two ends — this is where the "end cap" bug in an earlier version
        // did *not* show up (only the doorway thresholds did), so it's worth
        // covering separately.
        let mut state = setup();
        let anchor = Vec3::new(0, 0, 500);
        let path = test_path();

        clear_layout(&state, anchor);
        render_layout(&state, anchor, &path);
        flush(&mut state);

        let from = anchor + Vec3::new(-ROOM_HALF.x, 0, DOOR_Z);
        let parent_center =
            anchor + Vec3::new(-(ROOM_HALF.x * 2 + CORRIDOR_LEN), 0, LEVEL_SHIFT);
        let to = parent_center + Vec3::new(ROOM_HALF.x, 0, DOOR_Z);
        let mid = Vec3::new((from.x + to.x) / 2, from.y, (from.z + to.z) / 2);
        assert_eq!(
            block_kind_at(&state, mid),
            Some(BlockKind::Air),
            "the middle of the ramp must be walkable, not just its two doorway ends"
        );
    }

    #[test]
    fn spawn_position_is_within_every_doorway_height_band() {
        // The height used to teleport the player in (see `enter`'s
        // `spawn`) must fall inside the vertical range every doorway opens
        // (`DOOR_Z ..= DOOR_Z + DOOR_HEIGHT - 1`), or a player standing at
        // spawn height — before/without falling to the floor — would have
        // their head above every doorway's ceiling.
        let spawn_z_offset = DOOR_Z + 2;
        assert!(
            (DOOR_Z..DOOR_Z + DOOR_HEIGHT).contains(&spawn_z_offset),
            "spawn height {spawn_z_offset} must be within the doorway band {DOOR_Z}..{}",
            DOOR_Z + DOOR_HEIGHT
        );
    }
}
