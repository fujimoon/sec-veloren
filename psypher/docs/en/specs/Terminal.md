# Terminal (Semi-Transparent, Real Terminal) Specification

Specification for a **semi-transparent terminal window backed by a real shell**, added to Veloren's (voxygen) debug egui UI.

![Terminal window in action](../../images/terminal.png)

Above: the "Debug Control" → "Terminal" checkbox enabled inside a running singleplayer world. A semi-transparent window overlays the game view, running a real `zsh` shell with a live prompt. The cursor renders as a solid white block because the window currently has keyboard focus.

## 1. Purpose

Give developers/debuggers instant access to a real shell without leaving the running game process — for checking logs, poking at files, or running quick scripts without switching windows.

- Intended for development/debugging only, not a player-facing feature.
- This is **not** a fake shell that parses a small set of commands — it spawns a real shell process attached to a real pty. ANSI escapes, colors, and TUI apps (e.g. vim) are expected to work as they would in any terminal emulator.

## 2. Usage

1. Enter a world (singleplayer or multiplayer) — the debug UI is disabled on the main menu.
2. Press the `ToggleEguiDebug` key (default: **F7**) to open the "Debug Control" window.
   - **macOS note**: laptop keyboards map F1–F12 to media keys by default, so a bare F7 press may do nothing. Either hold **fn + F7**, or enable "Use F1, F2, etc. keys as standard function keys" in System Settings → Keyboard.
3. Check the **"Terminal"** checkbox — a semi-transparent Terminal window opens with a real shell already running inside it.
4. Click inside the window to give it keyboard focus; subsequent keystrokes go to the shell instead of the game (game hotkeys are unaffected while it's focused).
5. Unchecking the box, or closing the window via the ✕, kills the shell process.

## 3. Architecture

Built on top of the existing `egui-ui` feature (enabled by default). No new render pipeline is required — it rides on egui's existing wgpu compositing.

```
Debug Control (existing egui window)
  └─ "Terminal" checkbox
       └─ TerminalState::new()   … spawns a pty + shell
            └─ TerminalState::show()  … every frame, paints the grid via egui::Painter
```

### Libraries used

| Library | Role |
|---|---|
| [`alacritty_terminal`](https://docs.rs/alacritty_terminal) 0.25 | The same pty-spawning (`tty::new`) and ANSI/VTE parsing + terminal grid state machinery that the Alacritty terminal emulator itself is built on — including the background thread that reads pty output and keeps `Term` up to date |
| `egui` (existing dependency) | Renders the semi-transparent window, captures keyboard/text input events, and draws the grid (rectangles + text) |

`egui` (0.33) and `alacritty_terminal` are independent crates — `alacritty_terminal` has no dependency on egui — so there's no version conflict between them.

### Spawning the shell

- `alacritty_terminal::tty::new` launches `$SHELL` (falls back to `/bin/zsh` on macOS by default).
- `TERM=xterm-256color` is injected into the child's environment.
- Starts at 80x24; afterwards, the column/row count is recomputed each frame from the measured monospace font metrics and the window's pixel size, and `Term::resize` + a pty `Msg::Resize` are only sent when that count actually changes.

### I/O

- **Output**: alacritty_terminal's own `EventLoop` runs a background thread that reads the pty, parses the ANSI stream, and updates the shared `Arc<FairMutex<Term<_>>>` directly. The rendering side just locks it and reads the grid once per frame.
- **Input**: egui's `Event::Text` / `Event::Paste` / `Event::Key` are collected only while the terminal widget holds keyboard focus, converted to raw bytes (printable text, `\r`, `\x7f`, arrow-key escape sequences, Ctrl+letter control codes, etc.), and sent to the pty via `Msg::Input`.

### Transparency

```rust
Frame::window(&ctx.style())
    .fill(Color32::from_rgba_unmultiplied(12, 12, 16, 200))
```

This rides on the alpha-blended compositing the existing egui-ui debug overlay already uses on top of the wgpu-rendered scene — no shader changes needed.

### Rendering

Walks `term.renderable_content().display_iter` cell by cell, drawing a background rect and glyph via `egui::Painter` for each. Colors resolve from `vte::ansi::Color` (Named/Indexed/Spec) to the standard xterm 16-color palette, the 216-color cube, and the 24-step grayscale ramp. `Flags::INVERSE` (reverse video), `Flags::BOLD` (brightens the named color), `Flags::*UNDERLINE`, and `Flags::HIDDEN` are all honored.

### Focus-aware cursor

Matches what a real terminal emulator does:

- **Focused**: the cell is filled solid white, with the character underneath redrawn in inverse color on top.
- **Unfocused**: only a hollow white outline is drawn.

This is driven by re-checking `Response::has_focus()` — from the `ui.allocate_exact_size(..., Sense::click_and_drag())` call — every frame; clicking the widget calls `request_focus()`.

### Avoiding conflicts with game controls

No extra code was needed for this: [`voxygen/src/run.rs`](../../../../voxygen/src/run.rs) already guards against forwarding any window event that egui has consumed on to the game's input handling. As soon as the terminal widget holds egui keyboard focus, that existing guard kicks in automatically.

## 4. Files changed

- [`voxygen/egui/src/terminal.rs`](../../../../voxygen/egui/src/terminal.rs) (new) — the implementation
- [`voxygen/egui/Cargo.toml`](../../../../voxygen/egui/Cargo.toml) — adds the `alacritty_terminal` dependency
- [`voxygen/egui/src/lib.rs`](../../../../voxygen/egui/src/lib.rs) — adds the "Terminal" checkbox to the "Debug Control" window, and a `terminal: Option<TerminalState>` field on `EguiInnerState` (lazily created when checked, dropped — killing the shell — when unchecked)

## 5. Known limitations (v1)

- No scrollback (mouse-wheel scroll-up history) — only the current viewport is ever shown.
- Some `PtyWrite` events — e.g. terminal-capability queries some programs send (DA/OSC) — are silently dropped (`VoidListener` ignores them), which may cause a subset of TUI-app behavior to not work perfectly.
- If the shell process exits, the display just freezes on its last frame — there's no exit detection or "restart" affordance yet.
- No mouse support (selection, scrolling, clickable URLs, etc.) — keyboard input only.

## 6. Security posture

- This is a client-local debug feature living under the `egui-ui` feature; it is not reachable over the network or by the server.
- Because it spawns a real shell, it is not intended to be shipped to end players in a distributed build — it assumes a developer running the game locally.
