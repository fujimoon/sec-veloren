//! 2-D ASCII dungeon that mirrors the filesystem hierarchy.
//!
//! Layout (80 columns × 24 rows minimum):
//!
//!  col   0─15  │ parent directory room   (16 wide)
//!  col  16─19  │ left corridor  ▲ stairs ( 4 wide)
//!  col  20─59  │ current room            (40 wide)
//!  col  60─63  │ right corridor ▼ stairs ( 4 wide)
//!  col  64─79  │ child hallway / room    (16 wide)
//!
//!  row   0    : status bar (shows current path)
//!  rows  1─22 : dungeon rooms
//!  row  23    : help bar
//!  row  11    : corridor connection row (CORR)
//!
//! Controls:
//!   ↑ ↓ ← →    Move player
//!   < (or walk left at CORR row) : ascend to parent directory
//!   > (or walk right at CORR row): descend to child directory
//!   Enter (in hallway)           : enter highlighted child directory
//!   q / Esc                      : quit

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor,
        SetForegroundColor,
    },
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

// ── Layout constants ──────────────────────────────────────────────────────────

const PL: u16 = 0;  const PR: u16 = 15; // parent room cols (inclusive)
const LL: u16 = 16; const _LR: u16 = 19; // left corridor cols
const CL: u16 = 20; const CR: u16 = 59; // current room cols
const RL: u16 = 60; const _RR: u16 = 63; // right corridor cols
const HL: u16 = 64; const HR: u16 = 79; // hallway/child area cols

const ROOM_T: u16 = 1;  // room top wall row
const ROOM_B: u16 = 22; // room bottom wall row
const INT_T: u16 = 2;   // first interior row
const INT_B: u16 = 21;  // last interior row
const CORR: u16 = 11;   // corridor connection row

// Max children shown before "+N more" indicator
// Rows available: INT_T+2 .. INT_B  →  4..21  →  18 rows; keep 17 + 1 for "+N more"
const MAX_HALL: usize = 17;

// ── State ─────────────────────────────────────────────────────────────────────

struct State {
    cwd: PathBuf,
    parent: Option<PathBuf>,
    children: Vec<PathBuf>, // non-hidden subdirs, sorted

    // Player position inside the current room (meaningful only when !hallway)
    px: u16,
    py: u16,

    // Hallway mode: player has entered the child area
    hallway: bool,
    sel: usize, // selected child index in hallway
}

impl State {
    fn new() -> io::Result<Self> {
        let cwd = std::env::current_dir()?;
        let (parent, children) = scan(&cwd);
        Ok(Self {
            cwd,
            parent,
            children,
            px: (CL + CR) / 2,
            py: INT_T + 4, // start a few rows below top, well away from CORR
            hallway: false,
            sel: 0,
        })
    }

    fn cd_parent(&mut self) {
        let Some(p) = self.parent.clone() else { return };
        if std::env::set_current_dir(&p).is_ok() {
            self.cwd = p;
            self.reload();
        }
    }

    fn cd_child(&mut self, idx: usize) {
        let Some(c) = self.children.get(idx).cloned() else { return };
        if std::env::set_current_dir(&c).is_ok() {
            self.cwd = c;
            self.reload();
        }
    }

    fn reload(&mut self) {
        let (parent, children) = scan(&self.cwd);
        self.parent = parent;
        self.children = children;
        self.px = (CL + CR) / 2;
        self.py = INT_T + 4;
        self.hallway = false;
        self.sel = 0;
    }
}

/// Directory listing (parent + sorted, non-hidden children), delegated to the
/// shared `psypher-os-dungeon-game` crate — the same directory-scanning logic
/// (hidden-dir exclusion, sorting, unreadable-entries-skipped-not-errored)
/// backs both this ASCII browser and the in-game 3D OS Dungeon feature, so it
/// only needs to be gotten right in one place.
fn scan(path: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    let parent = path.parent().map(PathBuf::from);
    let children = psypher_os_dungeon_game::list_children(path)
        .into_iter()
        .map(|d| d.full_path)
        .collect();
    (parent, children)
}

// ── Text helpers ──────────────────────────────────────────────────────────────

/// Last path component, or "/" for the filesystem root.
fn basename(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string())
}

/// Truncate `s` to at most `max` Unicode code-points, appending "…" if cut.
fn fit(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = chars[..max.saturating_sub(1)].iter().collect();
        out.push('…');
        out
    }
}

/// Like `fit` but also pads with spaces on the right to exactly `width` chars.
fn fitpad(s: &str, width: usize) -> String {
    let truncated = fit(s, width);
    let n = truncated.chars().count();
    if n < width {
        format!("{}{}", truncated, " ".repeat(width - n))
    } else {
        truncated
    }
}

// ── Input ─────────────────────────────────────────────────────────────────────

/// Process one event. Returns `true` if the user asked to quit.
fn handle(s: &mut State) -> io::Result<bool> {
    if !event::poll(std::time::Duration::from_millis(50))? {
        return Ok(false);
    }
    let ev = event::read()?;
    let Event::Key(k) = ev else { return Ok(false) };
    if k.kind != KeyEventKind::Press {
        return Ok(false);
    }

    match k.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),

        // Quick staircase actions (roguelike style)
        KeyCode::Char('<') => s.cd_parent(),
        KeyCode::Char('>') => enter_child_area(s),

        // Movement
        KeyCode::Up => do_move(s, 0, -1),
        KeyCode::Down => do_move(s, 0, 1),
        KeyCode::Left => do_move(s, -1, 0),
        KeyCode::Right => do_move(s, 1, 0),

        // Enter while in hallway: descend into selected child
        KeyCode::Enter => {
            if s.hallway {
                let idx = s.sel;
                s.cd_child(idx);
            }
        },
        _ => {},
    }
    Ok(false)
}

/// Move player by (dx, dy), handling wall collisions and staircase transitions.
fn do_move(s: &mut State, dx: i16, dy: i16) {
    // ── Hallway mode ──────────────────────────────────────────────────────────
    if s.hallway {
        match (dx, dy) {
            (-1, 0) => {
                // Exit hallway: place player near the right wall of current room
                s.hallway = false;
                s.px = CR - 1;
                s.py = CORR;
            },
            (1, 0) => {
                let idx = s.sel;
                s.cd_child(idx);
            },
            (0, -1) => {
                if s.sel > 0 {
                    s.sel -= 1;
                }
            },
            (0, 1) => {
                let max_sel = s.children.len().min(MAX_HALL).saturating_sub(1);
                if s.sel < max_sel {
                    s.sel += 1;
                }
            },
            _ => {},
        }
        return;
    }

    // ── Current room mode ─────────────────────────────────────────────────────
    // Vertical movement: clamp to interior rows
    if dy != 0 {
        let ny = (s.py as i16 + dy) as u16;
        if ny >= INT_T && ny <= INT_B {
            s.py = ny;
        }
        return;
    }

    // Horizontal movement
    let nx = (s.px as i16 + dx) as u16;

    if dx < 0 {
        if nx >= CL + 1 {
            // Move freely within interior
            s.px = nx;
        } else if s.py == CORR && s.parent.is_some() {
            // At left wall on corridor row → ascend
            s.cd_parent();
        }
        // else: wall blocks (no parent or not on corridor row)
    } else if dx > 0 {
        if nx <= CR - 1 {
            s.px = nx;
        } else if s.py == CORR && !s.children.is_empty() {
            // At right wall on corridor row → enter child area
            enter_child_area(s);
        }
    }
}

fn enter_child_area(s: &mut State) {
    if s.children.is_empty() {
        return;
    }
    if s.children.len() == 1 {
        s.cd_child(0);
    } else {
        s.hallway = true;
        s.sel = 0;
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(s: &State, out: &mut impl Write) -> io::Result<()> {
    queue!(out, Clear(ClearType::All))?;

    draw_status(s, out)?;
    draw_help(out)?;
    draw_parent_room(s, out)?;
    draw_current_room(s, out)?;
    draw_child_area(s, out)?;

    // Corridors are drawn last so they overwrite room wall chars with openings
    if s.parent.is_some() {
        draw_left_corridor(out)?;
    }
    if !s.children.is_empty() {
        draw_right_corridor(out)?;
    }

    draw_player(s, out)?;

    out.flush()
}

// ─ Status bar ─────────────────────────────────────────────────────────────────

fn draw_status(s: &State, out: &mut impl Write) -> io::Result<()> {
    let label = format!(" PSY DUNGEON  {}  ", s.cwd.display());
    queue!(
        out,
        MoveTo(0, 0),
        SetBackgroundColor(Color::DarkBlue),
        SetForegroundColor(Color::White),
        SetAttribute(Attribute::Bold),
        Print(fitpad(&label, 80)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )
}

// ─ Help bar ───────────────────────────────────────────────────────────────────

fn draw_help(out: &mut impl Write) -> io::Result<()> {
    let msg =
        " Arrows:Move  <:Ascend  >:Descend  Enter:Enter room  q:Quit";
    queue!(
        out,
        MoveTo(0, 23),
        SetBackgroundColor(Color::DarkGrey),
        SetForegroundColor(Color::Grey),
        Print(fitpad(msg, 80)),
        ResetColor,
    )
}

// ─ Box drawing ────────────────────────────────────────────────────────────────

/// Draw a rectangular box using box-drawing characters.
fn draw_box(out: &mut impl Write, col: u16, row: u16, w: u16, h: u16, c: Color) -> io::Result<()> {
    let inner = w.saturating_sub(2) as usize;
    let hbar: String = "─".repeat(inner);

    queue!(out, SetForegroundColor(c))?;
    // Top border
    queue!(out, MoveTo(col, row), Print(format!("┌{}┐", hbar)))?;
    // Bottom border
    queue!(out, MoveTo(col, row + h - 1), Print(format!("└{}┘", hbar)))?;
    // Side walls
    for r in row + 1..row + h - 1 {
        queue!(out, MoveTo(col, r), Print("│"))?;
        queue!(out, MoveTo(col + w - 1, r), Print("│"))?;
    }
    queue!(out, ResetColor)
}

// ─ Parent room ────────────────────────────────────────────────────────────────

fn draw_parent_room(s: &State, out: &mut impl Write) -> io::Result<()> {
    let (col, w, h) = (PL, PR - PL + 1, ROOM_B - ROOM_T + 1); // 0, 16, 22

    if let Some(parent) = &s.parent {
        draw_box(out, col, ROOM_T, w, h, Color::DarkCyan)?;

        // Room title
        queue!(
            out,
            MoveTo(PL + 2, INT_T),
            SetForegroundColor(Color::Cyan),
            SetAttribute(Attribute::Bold),
            Print(fit("PARENT DIR", 12)),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )?;

        // Directory name (basename)
        let name = basename(parent);
        queue!(
            out,
            MoveTo(PL + 1, INT_T + 2),
            SetForegroundColor(Color::DarkCyan),
            Print(fit(&name, 14)),
            ResetColor,
        )?;

        // Small hint near corridor row
        queue!(
            out,
            MoveTo(PL + 1, CORR - 2),
            SetForegroundColor(Color::DarkGrey),
            Print(fit("< ascend", 14)),
            ResetColor,
        )?;
    } else {
        // No parent (filesystem root)
        draw_box(out, col, ROOM_T, w, h, Color::DarkGrey)?;
        queue!(
            out,
            MoveTo(PL + 2, CORR),
            SetForegroundColor(Color::DarkGrey),
            Print(fit("[root]", 12)),
            ResetColor,
        )?;
    }
    Ok(())
}

// ─ Current room ───────────────────────────────────────────────────────────────

fn draw_current_room(s: &State, out: &mut impl Write) -> io::Result<()> {
    let (col, w, h) = (CL, CR - CL + 1, ROOM_B - ROOM_T + 1); // 20, 40, 22

    draw_box(out, col, ROOM_T, w, h, Color::White)?;

    // Room name (bold, bright)
    let name = basename(&s.cwd);
    queue!(
        out,
        MoveTo(CL + 2, INT_T),
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print(fit(&name, 36)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // Subdirectory count hint
    let hint = match s.children.len() {
        0 => " no subdirectories".to_string(),
        1 => " 1 subdirectory  >".to_string(),
        n => format!(" {} subdirectories  >", n),
    };
    queue!(
        out,
        MoveTo(CL + 1, INT_T + 1),
        SetForegroundColor(Color::DarkGrey),
        Print(fit(&hint, 38)),
        ResetColor,
    )?;

    // Corridor row indicator labels (only when adjacent connections exist)
    if s.parent.is_some() {
        queue!(
            out,
            MoveTo(CL + 2, CORR),
            SetForegroundColor(Color::DarkGrey),
            Print(fit("< parent", 10)),
            ResetColor,
        )?;
    }
    if !s.children.is_empty() {
        queue!(
            out,
            MoveTo(CR - 12, CORR),
            SetForegroundColor(Color::DarkGrey),
            Print(fit("children >", 10)),
            ResetColor,
        )?;
    }

    Ok(())
}

// ─ Child / Hallway area ───────────────────────────────────────────────────────

fn draw_child_area(s: &State, out: &mut impl Write) -> io::Result<()> {
    let (col, w, h) = (HL, HR - HL + 1, ROOM_B - ROOM_T + 1); // 64, 16, 22

    if s.children.is_empty() {
        draw_box(out, col, ROOM_T, w, h, Color::DarkGrey)?;
        queue!(
            out,
            MoveTo(HL + 1, CORR),
            SetForegroundColor(Color::DarkGrey),
            Print(fit("(no subdirs)", 14)),
            ResetColor,
        )?;
        return Ok(());
    }

    let box_color = if s.hallway { Color::White } else { Color::DarkGreen };
    draw_box(out, col, ROOM_T, w, h, box_color)?;

    // Header
    let header = if s.children.len() == 1 { "CHILD DIR" } else { "HALLWAY" };
    let hdr_color = if s.hallway { Color::Green } else { Color::DarkGreen };
    queue!(
        out,
        MoveTo(HL + 2, INT_T),
        SetForegroundColor(hdr_color),
        SetAttribute(Attribute::Bold),
        Print(fit(header, 12)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // List children
    // Each entry occupies row INT_T+2+i  =  4, 5, 6, ...
    // Format at cols HL+1..HR-1 (14 chars):
    //   col HL+1 : '@' if selected by player, else ' '
    //   col HL+2 : '>'
    //   col HL+3 : ' '
    //   cols HL+4..HR-1 : dirname (11 chars)
    let shown = s.children.len().min(MAX_HALL);
    for (i, child) in s.children.iter().take(shown).enumerate() {
        let row = INT_T + 2 + i as u16;
        if row > INT_B {
            break;
        }
        let name = basename(child);
        let is_sel = s.hallway && i == s.sel;
        let marker = if is_sel { '@' } else { ' ' };
        let entry = format!("{}>  {}", marker, fit(&name, 11));
        let padded = fitpad(&entry, 14);

        if is_sel {
            queue!(
                out,
                MoveTo(HL + 1, row),
                SetBackgroundColor(Color::DarkYellow),
                SetForegroundColor(Color::Black),
                SetAttribute(Attribute::Bold),
                Print(&padded),
                ResetColor,
                SetAttribute(Attribute::Reset),
            )?;
        } else {
            let fc = if s.hallway { Color::White } else { Color::DarkGrey };
            queue!(
                out,
                MoveTo(HL + 1, row),
                SetForegroundColor(fc),
                Print(&padded),
                ResetColor,
            )?;
        }
    }

    // "+N more" if children overflow the display
    if s.children.len() > MAX_HALL {
        let overflow_row = INT_T + 2 + MAX_HALL as u16;
        if overflow_row <= INT_B {
            let more = format!("+{} more", s.children.len() - MAX_HALL);
            queue!(
                out,
                MoveTo(HL + 1, overflow_row),
                SetForegroundColor(Color::DarkGrey),
                Print(fit(&more, 14)),
                ResetColor,
            )?;
        }
    }

    Ok(())
}

// ─ Corridors ─────────────────────────────────────────────────────────────────

/// Draw the left corridor (between parent room and current room).
/// Overwrites room-wall characters at the corridor row with opening T-junctions.
fn draw_left_corridor(out: &mut impl Write) -> io::Result<()> {
    // Parent room right wall → opening east
    queue!(out, MoveTo(PR, CORR), SetForegroundColor(Color::DarkCyan), Print("├"), ResetColor)?;
    // Corridor floor: ─ ▲ ─ ─
    queue!(out, MoveTo(LL, CORR), SetForegroundColor(Color::DarkCyan), Print("─"), ResetColor)?;
    queue!(
        out,
        MoveTo(LL + 1, CORR),
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print("▲"),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;
    queue!(out, MoveTo(LL + 2, CORR), SetForegroundColor(Color::DarkCyan), Print("──"), ResetColor)?;
    // Current room left wall → opening west
    queue!(out, MoveTo(CL, CORR), SetForegroundColor(Color::DarkGrey), Print("┤"), ResetColor)
}

/// Draw the right corridor (between current room and child/hallway area).
fn draw_right_corridor(out: &mut impl Write) -> io::Result<()> {
    // Current room right wall → opening east
    queue!(out, MoveTo(CR, CORR), SetForegroundColor(Color::DarkGrey), Print("├"), ResetColor)?;
    // Corridor floor: ─ ─ ▼ ─
    queue!(out, MoveTo(RL, CORR), SetForegroundColor(Color::DarkGreen), Print("──"), ResetColor)?;
    queue!(
        out,
        MoveTo(RL + 2, CORR),
        SetForegroundColor(Color::Green),
        SetAttribute(Attribute::Bold),
        Print("▼"),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;
    queue!(out, MoveTo(RL + 3, CORR), SetForegroundColor(Color::DarkGreen), Print("─"), ResetColor)?;
    // Child area left wall → opening west
    queue!(out, MoveTo(HL, CORR), SetForegroundColor(Color::DarkGrey), Print("┤"), ResetColor)
}

// ─ Player ─────────────────────────────────────────────────────────────────────

fn draw_player(s: &State, out: &mut impl Write) -> io::Result<()> {
    if s.hallway {
        // In hallway mode the '@' is drawn inline with the selected child row
        // (handled in draw_child_area). Nothing extra needed.
        return Ok(());
    }
    queue!(
        out,
        MoveTo(s.px, s.py),
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print("@"),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )
}

// ── Cleanup guard ─────────────────────────────────────────────────────────────

struct TermGuard;
impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run() -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    if cols < 80 || rows < 24 {
        eprintln!(
            "psy dungeon requires at least 80×24 (terminal is {}×{}). Please resize.",
            cols, rows
        );
        return Ok(());
    }

    let mut state = State::new()?;
    let mut out = io::stdout();

    execute!(out, EnterAlternateScreen, Hide)?;
    let _guard = TermGuard; // restores terminal on ANY exit path (including errors below)
    terminal::enable_raw_mode()?;

    render(&state, &mut out)?;

    loop {
        match handle(&mut state) {
            Ok(true) => break,
            Ok(false) => {},
            Err(e) => {
                drop(_guard);
                return Err(e);
            },
        }
        render(&state, &mut out)?;
    }

    // _guard drops here, restoring terminal.
    // Print the final cwd so a shell wrapper can cd into it:
    //   function psy() { command psy "$@"; newdir=$(cat /tmp/.psy_cwd 2>/dev/null); ... }
    let cwd = state.cwd.display().to_string();
    drop(_guard);
    println!("{}", cwd);
    Ok(())
}
