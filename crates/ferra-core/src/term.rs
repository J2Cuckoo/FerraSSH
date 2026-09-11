//! VT/UTF-8 emulator backed by alacritty_terminal (same engine as Alacritty).

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{point_to_viewport, Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone)]
struct Proxy {
    tx: Sender<Event>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

pub struct Emulator {
    term: Term<Proxy>,
    processor: Processor,
    events: Receiver<Event>,
    pending_writes: Vec<String>,
    title: String,
    cwd: String,
    clipboard: Option<String>,
    decode_pending: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermCell {
    pub ch: char,
    pub fg: u32,
    pub bg: u32,
    pub flags: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermLine {
    pub y: u16,
    pub cells: Vec<TermCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermFrame {
    pub cols: u16,
    pub rows: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_visible: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub mouse_sgr: bool,
    pub mouse_mode: bool,
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    pub lines: Vec<TermLine>,
    /// Lines the viewport is scrolled up from the live bottom.
    #[serde(default)]
    pub scroll_offset: u32,
    /// Scrollback lines currently stored (0 = nothing to scroll).
    #[serde(default)]
    pub scroll_max: u32,
}

impl Emulator {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut config = Config::default();
        config.scrolling_history = scrollback;
        let size = TermSize { columns: cols.max(2) as usize, screen_lines: rows.max(1) as usize };
        let term = Term::new(config, &size, Proxy { tx });
        Self {
            term,
            processor: Processor::new(),
            events: rx,
            pending_writes: Vec::new(),
            title: String::new(),
            cwd: String::new(),
            clipboard: None,
            decode_pending: Vec::new(),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize { columns: cols.max(2) as usize, screen_lines: rows.max(1) as usize };
        self.term.resize(size);
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        let utf8 = decode_pty_bytes(&mut self.decode_pending, bytes);
        self.processor.advance(&mut self.term, &utf8);
        absorb_osc7(&utf8, &mut self.cwd);
        self.drain_events();
    }

    pub fn scroll(&mut self, delta: i32) {
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta));
        }
    }

    pub fn scroll_to(&mut self, offset: u32) {
        let cur = self.term.grid().display_offset() as i32;
        let max = self.term.history_size() as i32;
        let target = (offset as i32).clamp(0, max);
        let delta = target - cur;
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta));
        }
    }

    pub fn take_pty_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_writes)
    }

    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Best-effort remote directory: prompt line, OSC 7 / title, then stored cwd.
    pub fn infer_cwd(&self) -> String {
        for line in self.recent_prompt_lines() {
            if let Some(p) = cwd_from_prompt_line(&line) {
                return p;
            }
        }
        if looks_like_cwd(self.cwd.trim()) {
            return self.cwd.trim().to_string();
        }
        cwd_from_title(self.title.trim()).unwrap_or_default()
    }

    fn recent_prompt_lines(&self) -> Vec<String> {
        let grid = self.term.grid();
        let cols = grid.columns();
        let cursor_line = self.term.renderable_content().cursor.point.line;
        let mut lines = Vec::new();
        for delta in 0..4 {
            let line = Line(cursor_line.0 - delta);
            if line < grid.topmost_line() {
                break;
            }
            let mut row = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[line][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                row.push(cell.c);
            }
            let row = row.trim_end().to_string();
            if !row.is_empty() {
                lines.push(row);
            }
        }
        lines
    }

    pub fn snapshot(&self) -> TermFrame {
        let cols = self.term.columns() as u16;
        let rows = self.term.screen_lines() as u16;
        let mode = *self.term.mode();
        let content = self.term.renderable_content();
        let offset = content.display_offset;
        let cursor = content.cursor.point;
        let cursor_vp = point_to_viewport(offset, cursor).filter(|p| p.line < rows as usize);
        let mut lines: Vec<TermLine> = (0..rows)
            .map(|y| TermLine { y, cells: Vec::with_capacity(cols as usize) })
            .collect();

        for indexed in content.display_iter {
            // History rows use negative grid lines; map them onto the visible viewport.
            let y = indexed.point.line.0 + offset as i32;
            if y < 0 {
                continue;
            }
            let y = y as usize;
            if y >= lines.len() {
                continue;
            }
            if indexed.cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            lines[y].cells.push(TermCell {
                ch: indexed.cell.c,
                fg: color_to_rgb(indexed.cell.fg, false),
                bg: color_to_rgb(indexed.cell.bg, true),
                flags: indexed.cell.flags.bits() as u16,
            });
        }

        TermFrame {
            cols,
            rows,
            cursor_x: cursor_vp.map(|p| p.column.0 as u16).unwrap_or(0),
            cursor_y: cursor_vp.map(|p| p.line as u16).unwrap_or(0),
            cursor_visible: mode.contains(TermMode::SHOW_CURSOR) && cursor_vp.is_some(),
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            app_keypad: mode.contains(TermMode::APP_KEYPAD),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            mouse_sgr: mode.contains(TermMode::SGR_MOUSE),
            mouse_mode: mode.intersects(TermMode::MOUSE_MODE),
            title: self.title.clone(),
            cwd: {
                let inferred = self.infer_cwd();
                if inferred.is_empty() {
                    self.cwd.clone()
                } else {
                    inferred
                }
            },
            lines,
            scroll_offset: offset as u32,
            scroll_max: self.term.history_size() as u32,
        }
    }

    pub fn all_text(&self) -> String {
        let grid = self.term.grid();
        let cols = grid.columns();
        let mut out = String::new();
        let mut line = grid.topmost_line();
        let last = grid.bottommost_line();
        while line <= last {
            let mut row = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[line][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                row.push(cell.c);
            }
            out.push_str(row.trim_end());
            out.push('\n');
            line = Line(line.0 + 1);
        }
        out
    }

    /// Copy text between two grid points. `line` uses alacritty coords (0 = live top, negative = history).
    /// `col` is the index among non-spacer cells on that row, matching the frontend snapshot.
    pub fn text_range(&self, a_line: i32, a_col: usize, b_line: i32, b_col: usize) -> String {
        let mut start = (a_line, a_col);
        let mut end = (b_line, b_col);
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        let grid = self.term.grid();
        let cols = grid.columns();
        let top = grid.topmost_line().0;
        let bot = grid.bottommost_line().0;
        start.0 = start.0.clamp(top, bot);
        end.0 = end.0.clamp(top, bot);
        let mut out = String::new();
        let mut line = start.0;
        while line <= end.0 {
            let mut cells = Vec::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[Line(line)][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                cells.push(cell.c);
            }
            if !cells.is_empty() {
                let last_i = cells.len() - 1;
                let from = if line == start.0 { start.1.min(last_i) } else { 0 };
                let to = if line == end.0 { end.1.min(last_i) } else { last_i };
                if from <= to {
                    let mut row = String::new();
                    for ch in &cells[from..=to] {
                        row.push(*ch);
                    }
                    out.push_str(row.trim_end());
                }
            }
            if line != end.0 {
                out.push('\n');
            }
            line += 1;
        }
        out
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    pub fn select_line(&mut self, y: usize) {
        let cols = self.term.columns();
        let start = Point::new(Line(y as i32), Column(0));
        let end = Point::new(Line(y as i32), Column(cols.saturating_sub(1)));
        let mut selection = alacritty_terminal::selection::Selection::new(
            alacritty_terminal::selection::SelectionType::Lines,
            start,
            alacritty_terminal::index::Side::Left,
        );
        selection.update(end, alacritty_terminal::index::Side::Right);
        self.term.selection = Some(selection);
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Title(title) => {
                    if let Some(cwd) = cwd_from_title(&title) {
                        self.cwd = cwd;
                    }
                    self.title = title;
                }
                Event::ResetTitle => self.title.clear(),
                Event::PtyWrite(data) => self.pending_writes.push(data),
                Event::ClipboardStore(_clipboard, data) => self.clipboard = Some(data),
                Event::Bell => {}
                _ => {}
            }
        }
    }
}

pub type SharedEmulator = std::sync::Arc<Mutex<Emulator>>;

pub fn cwd_from_title(title: &str) -> Option<String> {
    let title = title.trim();
    if looks_like_cwd(title) {
        return Some(title.to_string());
    }
    for sep in [" — ", " – ", " - "] {
        if let Some((_, right)) = title.rsplit_once(sep) {
            let right = right.trim();
            if looks_like_cwd(right) {
                return Some(right.to_string());
            }
        }
    }
    if let Some(idx) = title.rfind(':') {
        let rest = title[idx + 1..].trim();
        if looks_like_cwd(rest) {
            return Some(rest.to_string());
        }
    }
    None
}

fn looks_like_cwd(s: &str) -> bool {
    s.starts_with('/') || s.starts_with('~')
}

pub fn cwd_from_prompt_line(line: &str) -> Option<String> {
    let t = line.trim();
    let last = t.chars().last()?;
    if !matches!(last, '$' | '#' | '%' | '>') {
        return None;
    }
    let t = t.trim_end_matches(['$', '#', '%', '>']).trim();
    cwd_from_title(t)
}

/// Decode PTY bytes as UTF-8, falling back to GB18030/GBK for Chinese locales.
/// Incomplete sequences at the end of a chunk stay in `pending`.
fn decode_pty_bytes(pending: &mut Vec<u8>, incoming: &[u8]) -> Vec<u8> {
    pending.extend_from_slice(incoming);
    let mut out = Vec::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.extend_from_slice(s.as_bytes());
                pending.clear();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    out.extend_from_slice(&pending[..valid]);
                    pending.drain(..valid);
                    continue;
                }
                if e.error_len().is_none() {
                    break;
                }
                let n = gb18030_char_len(pending);
                if n == 0 {
                    break;
                }
                let (cow, _, _) = encoding_rs::GB18030.decode(&pending[..n]);
                out.extend_from_slice(cow.as_bytes());
                pending.drain(..n);
            }
        }
    }
    out
}

fn gb18030_char_len(buf: &[u8]) -> usize {
    let Some(&b0) = buf.first() else {
        return 0;
    };
    if b0 < 0x80 {
        return 1;
    }
    if !(0x81..=0xfe).contains(&b0) {
        return 1;
    }
    let Some(&b1) = buf.get(1) else {
        return 0;
    };
    if (0x30..=0x39).contains(&b1) {
        if buf.len() < 4 {
            return 0;
        }
        return 4;
    }
    if (0x40..=0x7e).contains(&b1) || (0x80..=0xfe).contains(&b1) {
        return 2;
    }
    1
}

fn absorb_osc7(bytes: &[u8], cwd: &mut String) {
    let text = String::from_utf8_lossy(bytes);
    let mut rest = text.as_ref();
    while let Some(i) = rest.find("\x1b]7;") {
        rest = &rest[i + 4..];
        let end = rest.find(['\u{7}', '\u{1b}']).unwrap_or(rest.len());
        if let Some(path) = path_from_file_uri(&rest[..end]) {
            *cwd = path;
        }
        rest = &rest[end.min(rest.len())..];
    }
}

fn path_from_file_uri(uri: &str) -> Option<String> {
    let uri = uri.trim();
    let path = uri.strip_prefix("file://")?;
    let path = match path.find('/') {
        Some(0) => path,
        Some(i) => &path[i..],
        None => return None,
    };
    if path.is_empty() {
        None
    } else {
        Some(percent_decode(path))
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

const XTERM: [u32; 256] = xterm_palette();

const fn cube(n: usize) -> u32 {
    if n == 0 { 0 } else { 55 + 40 * n as u32 }
}

const fn xterm_palette() -> [u32; 256] {
    let mut p = [0u32; 256];
    p[0] = 0x000000;
    p[1] = 0xcd3131;
    p[2] = 0x0dbc79;
    p[3] = 0xe5e510;
    p[4] = 0x2472c8;
    p[5] = 0xbc3fbc;
    p[6] = 0x11a8cd;
    p[7] = 0xe5e5e5;
    p[8] = 0x666666;
    p[9] = 0xf14c4c;
    p[10] = 0x23d18b;
    p[11] = 0xf5f543;
    p[12] = 0x3b8eea;
    p[13] = 0xd670d6;
    p[14] = 0x29b8db;
    p[15] = 0xe5e5e5;
    let mut i = 0;
    while i < 216 {
        let r = i / 36;
        let g = (i / 6) % 6;
        let b = i % 6;
        p[16 + i] = (cube(r) << 16) | (cube(g) << 8) | cube(b);
        i += 1;
    }
    i = 0;
    while i < 24 {
        let v = 8 + 10 * i as u32;
        p[232 + i] = (v << 16) | (v << 8) | v;
        i += 1;
    }
    p
}

fn color_to_rgb(color: Color, background: bool) -> u32 {
    match color {
        Color::Named(name) => named(name, background),
        Color::Indexed(idx) => XTERM[idx as usize],
        Color::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
    }
}

fn named(name: NamedColor, background: bool) -> u32 {
    match name {
        NamedColor::Black => XTERM[0],
        NamedColor::Red => XTERM[1],
        NamedColor::Green => XTERM[2],
        NamedColor::Yellow => XTERM[3],
        NamedColor::Blue => XTERM[4],
        NamedColor::Magenta => XTERM[5],
        NamedColor::Cyan => XTERM[6],
        NamedColor::White => XTERM[7],
        NamedColor::BrightBlack => XTERM[8],
        NamedColor::BrightRed => XTERM[9],
        NamedColor::BrightGreen => XTERM[10],
        NamedColor::BrightYellow => XTERM[11],
        NamedColor::BrightBlue => XTERM[12],
        NamedColor::BrightMagenta => XTERM[13],
        NamedColor::BrightCyan => XTERM[14],
        NamedColor::BrightWhite => XTERM[15],
        NamedColor::Foreground => 0xd6deeb,
        NamedColor::Background => 0x0b0f14,
        NamedColor::Cursor => 0x3dcdc3,
        _ => {
            if background {
                0x0b0f14
            } else {
                0xd6deeb
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sgr_truecolor_and_utf8() {
        let mut em = Emulator::new(40, 8, 100);
        em.advance(b"\x1b[38;2;255;128;0mHi \xE4\xB8\xAD\xE6\x96\x87 \xF0\x9F\x9A\x80");
        let frame = em.snapshot();
        let line = &frame.lines[0];
        assert!(line.cells.iter().any(|c| c.ch == 'H'));
        assert!(line.cells.iter().any(|c| c.ch == '中'));
    }

    #[test]
    fn decodes_gbk_chinese() {
        let mut em = Emulator::new(40, 8, 100);
        em.advance(&[0xc4, 0xe3, 0xba, 0xc3]); // GBK 「你好」
        let frame = em.snapshot();
        let line = &frame.lines[0];
        assert!(line.cells.iter().any(|c| c.ch == '你'));
        assert!(line.cells.iter().any(|c| c.ch == '好'));
        assert!(line.cells.iter().any(|c| c.flags & 0b0010_0000 != 0));
    }

    #[test]
    fn snapshot_keeps_text_when_scrolled() {
        let mut em = Emulator::new(20, 4, 100);
        for i in 0..12 {
            em.advance(format!("LINE{i:02}\r\n").as_bytes());
        }
        let bottom = em.snapshot();
        assert_eq!(bottom.scroll_offset, 0);
        assert!(bottom.scroll_max > 0);
        let bottom_text: String = bottom
            .lines
            .iter()
            .flat_map(|l| l.cells.iter().map(|c| c.ch))
            .collect();
        assert!(bottom_text.contains("LINE"), "live view should show recent lines, got {bottom_text:?}");

        em.scroll(6);
        let up = em.snapshot();
        assert!(up.scroll_offset > 0, "scroll should move into history");
        let up_text: String = up
            .lines
            .iter()
            .flat_map(|l| l.cells.iter().map(|c| c.ch))
            .collect();
        assert!(
            up_text.chars().any(|c| !c.is_whitespace()),
            "scrolled viewport must keep visible cells, got {up_text:?}"
        );
        assert!(up_text.contains("LINE"), "scrolled view should still show history text, got {up_text:?}");
    }

    #[test]
    fn range_text_reads_history_after_scroll() {
        let mut em = Emulator::new(20, 4, 100);
        for i in 0..12 {
            em.advance(format!("LINE{i:02}\r\n").as_bytes());
        }
        let max = em.snapshot().scroll_max;
        assert!(max > 0);
        em.scroll_to(max);
        let top = em.snapshot();
        let abs_y = 0i32 - top.scroll_offset as i32;
        let first: String = top.lines[0].cells.iter().map(|c| c.ch).collect();
        let needle = first.trim();
        em.scroll_to(0);
        let copied = em.text_range(abs_y, 0, abs_y, 19);
        assert!(
            copied.contains(needle),
            "range copy should keep history text {needle:?}, got {copied:?}"
        );
    }

    #[test]
    fn title_and_osc7_set_cwd() {
        assert_eq!(
            cwd_from_title("root@kk-host:/opt/cangyou/qst").as_deref(),
            Some("/opt/cangyou/qst")
        );
        assert_eq!(
            cwd_from_title("root@kk-host: /opt/cangyou/qst").as_deref(),
            Some("/opt/cangyou/qst")
        );
        assert_eq!(cwd_from_title("user@host:~/src").as_deref(), Some("~/src"));
        assert_eq!(cwd_from_prompt_line("root@host:/var/log# ").as_deref(), Some("/var/log"));
        assert_eq!(cwd_from_prompt_line("user@host:~/proj $").as_deref(), Some("~/proj"));
        assert_eq!(cwd_from_prompt_line("cat /etc/passwd"), None);
        let mut em = Emulator::new(40, 8, 100);
        em.advance(b"\x1b]7;file://host/opt/cangyou/qst\x07");
        assert_eq!(em.cwd(), "/opt/cangyou/qst");
        em.advance(b"\x1b]0;root@kk-host: /tmp/work\x07");
        assert_eq!(em.infer_cwd(), "/tmp/work");
        let mut prompt = Emulator::new(40, 8, 100);
        prompt.advance(b"root@host:/var/log# ");
        assert_eq!(prompt.infer_cwd(), "/var/log");
    }
}
