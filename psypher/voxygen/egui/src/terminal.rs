//! A real, interactive terminal embedded in the debug egui overlay.
//!
//! This is not a re-implementation of a shell or a fake console: it opens an actual
//! pseudo-terminal (via [`alacritty_terminal`]'s `tty`/`event_loop` machinery, the
//! same building blocks the Alacritty terminal emulator itself is built on) and
//! spawns the user's real login shell (`$SHELL`, or `/bin/zsh` on macOS) attached to
//! it. Keystrokes typed into the egui window are written to the pty, and whatever the
//! shell/programs running in it print (including full ANSI/VTE escape sequences) are
//! parsed by a background thread and rendered as a proper terminal grid.
//!
//! Intended purely as a developer/debug convenience - see the "Terminal" checkbox in
//! the "Debug Control" window. It is gated the same way as the rest of `egui-ui` and
//! is never reachable by anything the server or other players control.

use std::{borrow::Cow, sync::Arc};

use alacritty_terminal::{
    event::{VoidListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config as TermConfig, Term, cell::Flags},
    tty::{self, Options as PtyOptions},
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb as AnsiRgb},
};
use egui::{
    Align2, Color32, Context, CornerRadius, FontId, Frame, Key, Margin, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};

const FONT_SIZE: f32 = 14.0;
const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 24;

/// Minimal [`Dimensions`] impl so we don't need alacritty's `term::test::TermSize`.
struct CellGrid {
    cols: usize,
    rows: usize,
}

impl Dimensions for CellGrid {
    fn total_lines(&self) -> usize { self.rows }

    fn screen_lines(&self) -> usize { self.rows }

    fn columns(&self) -> usize { self.cols }
}

/// State for one embedded terminal session (one real shell process + pty).
pub struct TerminalState {
    term: Arc<FairMutex<Term<VoidListener>>>,
    notifier: EventLoopSender,
    cols: usize,
    rows: usize,
    /// Measured monospace cell size in points, filled in on first draw.
    cell_size: Option<Vec2>,
}

impl TerminalState {
    /// Spawn a new shell in a new pty. Returns `None` (logging the error) if the pty
    /// or shell process could not be created.
    pub fn new() -> Option<Self> {
        let cols = DEFAULT_COLS;
        let rows = DEFAULT_ROWS;

        let mut options = PtyOptions::default();
        options
            .env
            .insert("TERM".to_owned(), "xterm-256color".to_owned());

        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: 8,
            cell_height: 16,
        };

        let pty = match tty::new(&options, window_size, 0) {
            Ok(pty) => pty,
            Err(err) => {
                tracing::error!("Failed to open embedded terminal pty: {err}");
                return None;
            },
        };

        let term = Term::new(
            TermConfig::default(),
            &CellGrid { cols, rows },
            VoidListener,
        );
        let term = Arc::new(FairMutex::new(term));

        let event_loop = match EventLoop::new(term.clone(), VoidListener, pty, false, false) {
            Ok(event_loop) => event_loop,
            Err(err) => {
                tracing::error!("Failed to start embedded terminal event loop: {err}");
                return None;
            },
        };
        let notifier = event_loop.channel();
        // The reader thread runs for the lifetime of the shell; we don't need to join
        // it, it exits on its own once the shell process exits or `Msg::Shutdown` is
        // sent (see `Drop` below).
        let _ = event_loop.spawn();

        Some(Self {
            term,
            notifier,
            cols,
            rows,
            cell_size: None,
        })
    }

    /// Draw the terminal window. `open` follows the usual egui checkbox convention:
    /// set it to `false` (e.g. via the window's close button) to have the caller drop
    /// this state next frame.
    pub fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new("Terminal")
            .open(open)
            .default_width(720.0)
            .default_height(420.0)
            .frame(
                Frame::window(&ctx.style())
                    .fill(Color32::from_rgba_unmultiplied(12, 12, 16, 200))
                    .inner_margin(Margin::same(6)),
            )
            .show(ctx, |ui| {
                let font_id = FontId::monospace(FONT_SIZE);
                let cell_size = *self.cell_size.get_or_insert_with(|| {
                    ui.fonts_mut(|fonts| {
                        let width = fonts.glyph_width(&font_id, 'M');
                        let height = fonts.row_height(&font_id);
                        Vec2::new(width, height)
                    })
                });

                let available = ui.available_size();
                let cols = ((available.x / cell_size.x).floor() as usize).max(4);
                let rows = ((available.y / cell_size.y).floor() as usize).max(2);
                self.resize(cols, rows);

                let desired = Vec2::new(cols as f32 * cell_size.x, rows as f32 * cell_size.y);
                let (rect, response) =
                    ui.allocate_exact_size(desired, Sense::click_and_drag());
                let has_focus = response.has_focus();
                if response.clicked() || response.dragged() {
                    response.request_focus();
                }

                self.draw_grid(ui, rect, cell_size, font_id.clone(), has_focus);

                if has_focus {
                    self.forward_input(ui);
                } else if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
            });
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;

        self.term.lock().resize(CellGrid { cols, rows });
        let _ = self.notifier.send(Msg::Resize(WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: 8,
            cell_height: 16,
        }));
    }

    fn draw_grid(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        cell_size: Vec2,
        font_id: FontId,
        has_focus: bool,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(8, 8, 10));

        let term = self.term.lock();
        let content = term.renderable_content();
        let bold_bright = true;

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = indexed.cell;
            if point.line.0 < 0 {
                continue;
            }
            let col = point.column.0;
            let line = point.line.0 as usize;
            if col >= self.cols || line >= self.rows {
                continue;
            }

            let inverse = cell.flags.contains(Flags::INVERSE);
            let bold = cell.flags.contains(Flags::BOLD);
            let (mut fg, mut bg) = (
                resolve_color(cell.fg, bold && bold_bright),
                resolve_color(cell.bg, false),
            );
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            let cell_pos = rect.min
                + Vec2::new(col as f32 * cell_size.x, line as f32 * cell_size.y);
            let cell_rect = Rect::from_min_size(cell_pos, cell_size);

            if bg != Color32::from_rgb(8, 8, 10) {
                painter.rect_filled(cell_rect, CornerRadius::ZERO, bg);
            }

            if !cell.flags.contains(Flags::HIDDEN) && cell.c != ' ' {
                painter.text(
                    cell_pos,
                    Align2::LEFT_TOP,
                    cell.c,
                    font_id.clone(),
                    fg,
                );
            }

            if cell.flags.intersects(Flags::ALL_UNDERLINES) {
                let y = cell_rect.max.y - 1.0;
                painter.line_segment(
                    [egui::pos2(cell_rect.min.x, y), egui::pos2(cell_rect.max.x, y)],
                    Stroke::new(1.0f32, fg),
                );
            }
        }

        // Cursor. A solid filled block means "this terminal has keyboard focus, keys
        // you type go here"; a hollow outline (like a real terminal emulator uses)
        // means it's visible but not receiving input.
        let cursor = content.cursor;
        if cursor.point.line.0 >= 0 {
            let line = cursor.point.line.0 as usize;
            let col = cursor.point.column.0;
            if col < self.cols && line < self.rows && cursor.shape != CursorShape::Hidden {
                let cell_pos = rect.min
                    + Vec2::new(col as f32 * cell_size.x, line as f32 * cell_size.y);
                let cell_rect = Rect::from_min_size(cell_pos, cell_size);
                match cursor.shape {
                    CursorShape::Underline => {
                        painter.line_segment(
                            [
                                egui::pos2(cell_rect.min.x, cell_rect.max.y - 1.0),
                                egui::pos2(cell_rect.max.x, cell_rect.max.y - 1.0),
                            ],
                            Stroke::new(2.0f32, Color32::WHITE),
                        );
                    },
                    CursorShape::Beam => {
                        painter.line_segment(
                            [cell_rect.min, egui::pos2(cell_rect.min.x, cell_rect.max.y)],
                            Stroke::new(2.0f32, Color32::WHITE),
                        );
                    },
                    _ if has_focus => {
                        // Block cursor, focused: filled, with the character underneath
                        // re-drawn in inverse video so it stays legible.
                        let under = &term.grid()[cursor.point];
                        painter.rect_filled(cell_rect, CornerRadius::ZERO, Color32::WHITE);
                        if under.c != ' ' {
                            painter.text(
                                cell_pos,
                                Align2::LEFT_TOP,
                                under.c,
                                font_id.clone(),
                                Color32::from_rgb(8, 8, 10),
                            );
                        }
                    },
                    _ => {
                        painter.rect_stroke(
                            cell_rect,
                            CornerRadius::ZERO,
                            Stroke::new(1.5f32, Color32::WHITE),
                            StrokeKind::Inside,
                        );
                    },
                };
            }
        }
    }

    /// Consume this frame's keyboard/text egui events and forward them to the pty as
    /// raw bytes, the same way a real terminal emulator would.
    fn forward_input(&self, ui: &egui::Ui) {
        let events = ui.ctx().input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => self.write(text.as_bytes()),
                egui::Event::Paste(text) => self.write(text.as_bytes()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = key_to_bytes(key, modifiers) {
                        self.write(&bytes);
                    }
                },
                _ => {},
            }
        }
    }

    fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if let Err(err) = self.notifier.send(Msg::Input(Cow::Owned(bytes.to_vec()))) {
            // Most likely the shell process has already exited and the reader
            // thread with it - log it once loudly rather than swallowing every
            // subsequent keystroke silently.
            tracing::warn!("Embedded terminal: failed to write input to pty: {err}");
        }
    }
}

impl Drop for TerminalState {
    fn drop(&mut self) { let _ = self.notifier.send(Msg::Shutdown); }
}

/// Map an egui key press (with modifiers) to the bytes a real terminal would send.
fn key_to_bytes(key: Key, modifiers: egui::Modifiers) -> Option<Vec<u8>> {
    if modifiers.ctrl || modifiers.mac_cmd {
        // Ctrl+<letter> -> the corresponding C0 control code, e.g. Ctrl+C -> 0x03.
        let name = format!("{key:?}");
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_uppercase();
            if c.is_ascii_uppercase() {
                return Some(vec![c & 0x1f]);
            }
        }
    }

    Some(match key {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Tab => b"\t".to_vec(),
        Key::Escape => vec![0x1b],
        Key::ArrowUp => b"\x1b[A".to_vec(),
        Key::ArrowDown => b"\x1b[B".to_vec(),
        Key::ArrowRight => b"\x1b[C".to_vec(),
        Key::ArrowLeft => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        _ => return None,
    })
}

fn resolve_color(color: AnsiColor, to_bright: bool) -> Color32 {
    match color {
        AnsiColor::Spec(rgb) => rgb_to_color32(rgb),
        AnsiColor::Indexed(idx) => indexed_to_color32(idx),
        AnsiColor::Named(named) => {
            let named = if to_bright { named.to_bright() } else { named };
            named_to_color32(named)
        },
    }
}

fn rgb_to_color32(rgb: AnsiRgb) -> Color32 { Color32::from_rgb(rgb.r, rgb.g, rgb.b) }

fn named_to_color32(named: NamedColor) -> Color32 {
    match named {
        NamedColor::Black | NamedColor::DimBlack => Color32::from_rgb(0x00, 0x00, 0x00),
        NamedColor::Red | NamedColor::DimRed => Color32::from_rgb(0xcd, 0x00, 0x00),
        NamedColor::Green | NamedColor::DimGreen => Color32::from_rgb(0x00, 0xcd, 0x00),
        NamedColor::Yellow | NamedColor::DimYellow => Color32::from_rgb(0xcd, 0xcd, 0x00),
        NamedColor::Blue | NamedColor::DimBlue => Color32::from_rgb(0x1e, 0x90, 0xff),
        NamedColor::Magenta | NamedColor::DimMagenta => Color32::from_rgb(0xcd, 0x00, 0xcd),
        NamedColor::Cyan | NamedColor::DimCyan => Color32::from_rgb(0x00, 0xcd, 0xcd),
        NamedColor::White | NamedColor::DimWhite => Color32::from_rgb(0xe5, 0xe5, 0xe5),
        NamedColor::BrightBlack => Color32::from_rgb(0x7f, 0x7f, 0x7f),
        NamedColor::BrightRed => Color32::from_rgb(0xff, 0x00, 0x00),
        NamedColor::BrightGreen => Color32::from_rgb(0x00, 0xff, 0x00),
        NamedColor::BrightYellow => Color32::from_rgb(0xff, 0xff, 0x00),
        NamedColor::BrightBlue => Color32::from_rgb(0x5c, 0x5c, 0xff),
        NamedColor::BrightMagenta => Color32::from_rgb(0xff, 0x00, 0xff),
        NamedColor::BrightCyan => Color32::from_rgb(0x00, 0xff, 0xff),
        NamedColor::BrightWhite => Color32::from_rgb(0xff, 0xff, 0xff),
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
            Color32::from_rgb(0xe5, 0xe5, 0xe5)
        },
        NamedColor::Background => Color32::from_rgb(0x08, 0x08, 0x0a),
        NamedColor::Cursor => Color32::WHITE,
    }
}

/// Standard xterm 256-color palette resolution (16 named + 216 color cube + 24
/// grayscale ramp).
fn indexed_to_color32(idx: u8) -> Color32 {
    match idx {
        0..=15 => named_to_color32(index_to_named(idx)),
        16..=231 => {
            let i = idx - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let scale = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(scale(r), scale(g), scale(b))
        },
        232..=255 => {
            let level = 8 + (idx - 232) * 10;
            Color32::from_rgb(level, level, level)
        },
    }
}

fn index_to_named(idx: u8) -> NamedColor {
    match idx {
        0 => NamedColor::Black,
        1 => NamedColor::Red,
        2 => NamedColor::Green,
        3 => NamedColor::Yellow,
        4 => NamedColor::Blue,
        5 => NamedColor::Magenta,
        6 => NamedColor::Cyan,
        7 => NamedColor::White,
        8 => NamedColor::BrightBlack,
        9 => NamedColor::BrightRed,
        10 => NamedColor::BrightGreen,
        11 => NamedColor::BrightYellow,
        12 => NamedColor::BrightBlue,
        13 => NamedColor::BrightMagenta,
        14 => NamedColor::BrightCyan,
        _ => NamedColor::BrightWhite,
    }
}
