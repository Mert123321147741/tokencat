//! Turns raw terminal bytes into the lines a human would have seen on screen.
//!
//! Stripping escape codes with a regex is not enough: progress bars redraw a
//! line with `\r`, and multi-line status areas (jest, vitest, cargo, docker)
//! move the cursor up and erase what they drew before. A naive stripper keeps
//! every intermediate frame. Instead we run a tiny line-oriented terminal
//! emulator that understands the cursor movement and erase sequences these
//! tools use, so only the final state of each line survives.

use std::collections::VecDeque;

/// Height of the virtual screen. Cursor-up movements cannot reach further
/// back than this, just like on a real terminal, which keeps a stray
/// `ESC[999A` from wiping the whole log.
pub const SCREEN_ROWS: usize = 50;

/// Width we advertise to child processes running under a PTY.
pub const SCREEN_COLS: usize = 200;

pub fn sanitize(input: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(input);
    let mut screen = Screen::new();
    screen.feed(&text);
    screen.into_lines()
}

struct Screen {
    /// Lines that scrolled off the virtual screen; they can no longer change.
    done: Vec<String>,
    /// The visible screen, at most `SCREEN_ROWS` lines.
    active: VecDeque<Vec<char>>,
    row: usize,
    col: usize,
    saved: (usize, usize),
}

impl Screen {
    fn new() -> Self {
        let mut active = VecDeque::new();
        active.push_back(Vec::new());
        Screen {
            done: Vec::new(),
            active,
            row: 0,
            col: 0,
            saved: (0, 0),
        }
    }

    fn feed(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\n' => self.newline(),
                '\r' => self.col = 0,
                '\x08' => self.col = self.col.saturating_sub(1),
                '\t' => self.put('\t'),
                '\x1b' => self.escape(&mut chars),
                '\u{9b}' => self.csi(&mut chars),
                c if (c as u32) < 0x20 || c == '\x7f' || ('\u{80}'..='\u{9f}').contains(&c) => {}
                c => self.put(c),
            }
        }
    }

    fn put(&mut self, c: char) {
        let line = &mut self.active[self.row];
        if self.col < line.len() {
            line[self.col] = c;
        } else {
            while line.len() < self.col {
                line.push(' ');
            }
            line.push(c);
        }
        self.col += 1;
    }

    fn newline(&mut self) {
        self.row += 1;
        self.col = 0;
        if self.row == self.active.len() {
            self.active.push_back(Vec::new());
        }
        self.scroll();
    }

    fn scroll(&mut self) {
        while self.active.len() > SCREEN_ROWS && self.row > 0 {
            let line = self.active.pop_front().unwrap();
            self.done.push(line.into_iter().collect());
            self.row -= 1;
            self.saved.0 = self.saved.0.saturating_sub(1);
        }
    }

    fn move_to_row(&mut self, row: usize) {
        while self.active.len() <= row && self.active.len() < SCREEN_ROWS {
            self.active.push_back(Vec::new());
        }
        self.row = row.min(self.active.len() - 1);
    }

    fn escape<I: Iterator<Item = char>>(&mut self, chars: &mut std::iter::Peekable<I>) {
        let Some(c) = chars.next() else { return };
        match c {
            '[' => self.csi(chars),
            // OSC, DCS, SOS, PM, APC: skip the payload up to BEL or ST.
            ']' | 'P' | 'X' | '^' | '_' => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || c == '\u{9c}' {
                        break;
                    }
                    if c == '\x1b' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Character set designation takes one more byte.
            '(' | ')' | '*' | '+' | '-' | '.' | '/' | '#' | '%' => {
                chars.next();
            }
            '7' => self.saved = (self.row, self.col),
            '8' => {
                let (r, c) = self.saved;
                self.move_to_row(r);
                self.col = c;
            }
            'M' => self.row = self.row.saturating_sub(1),
            'D' => self.move_to_row(self.row + 1),
            'E' => {
                self.newline();
            }
            _ => {}
        }
    }

    fn csi<I: Iterator<Item = char>>(&mut self, chars: &mut std::iter::Peekable<I>) {
        let mut params = String::new();
        let mut final_byte = None;
        for c in chars.by_ref() {
            match c {
                '0'..='?' | ' '..='/' => params.push(c),
                '@'..='~' => {
                    final_byte = Some(c);
                    break;
                }
                // Malformed sequence: drop it.
                _ => return,
            }
        }
        let Some(fin) = final_byte else { return };
        if params.starts_with(['?', '>', '<', '=']) {
            return; // private modes (cursor visibility, alt screen, ...)
        }
        let nums: Vec<usize> = params
            .split(';')
            .map(|p| {
                p.trim_matches(|c: char| !c.is_ascii_digit())
                    .parse()
                    .unwrap_or(0)
            })
            .collect();
        let n = |i: usize| nums.get(i).copied().filter(|&v| v > 0).unwrap_or(1);
        let raw = |i: usize| nums.get(i).copied().unwrap_or(0);
        match fin {
            'm' => {}
            'A' => self.row = self.row.saturating_sub(n(0)),
            'B' => self.move_to_row(self.row + n(0)),
            'C' => self.col += n(0),
            'D' => self.col = self.col.saturating_sub(n(0)),
            'E' => {
                self.move_to_row(self.row + n(0));
                self.col = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(n(0));
                self.col = 0;
            }
            'G' | '`' => self.col = n(0) - 1,
            'H' | 'f' => {
                self.move_to_row(n(0) - 1);
                self.col = n(1) - 1;
            }
            'K' => {
                let line = &mut self.active[self.row];
                match raw(0) {
                    0 => line.truncate(self.col),
                    1 => {
                        for c in line.iter_mut().take(self.col + 1) {
                            *c = ' ';
                        }
                    }
                    _ => line.clear(),
                }
            }
            'J' => match raw(0) {
                0 => {
                    self.active[self.row].truncate(self.col);
                    self.active.truncate(self.row + 1);
                }
                1 => {}
                // Clear screen: keep what was printed so far, start a fresh screen.
                _ => {
                    while let Some(line) = self.active.pop_front() {
                        if !line.is_empty() {
                            self.done.push(line.into_iter().collect());
                        }
                    }
                    self.active.push_back(Vec::new());
                    self.row = 0;
                    self.col = 0;
                }
            },
            's' => self.saved = (self.row, self.col),
            'u' => {
                let (r, c) = self.saved;
                self.move_to_row(r);
                self.col = c;
            }
            _ => {}
        }
    }

    fn into_lines(self) -> Vec<String> {
        let mut out = self.done;
        out.extend(self.active.into_iter().map(|l| l.into_iter().collect()));
        for line in out.iter_mut() {
            let trimmed = line.trim_end().len();
            line.truncate(trimmed);
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(input: &str) -> Vec<String> {
        sanitize(input.as_bytes())
    }

    #[test]
    fn strips_sgr_colors() {
        assert_eq!(s("\x1b[31mFAIL\x1b[0m src/a.ts\n"), vec!["FAIL src/a.ts"]);
    }

    #[test]
    fn carriage_return_keeps_final_progress_state() {
        let input = "Building [=>  ] 1/4\rBuilding [==> ] 2/4\rBuilding [====] 4/4\ndone\n";
        assert_eq!(s(input), vec!["Building [====] 4/4", "done"]);
    }

    #[test]
    fn erase_line_after_carriage_return() {
        let input = "downloading 100 files...\r\x1b[Kok\n";
        assert_eq!(s(input), vec!["ok"]);
    }

    #[test]
    fn crlf_is_a_plain_newline() {
        assert_eq!(s("a\r\nb\r\n"), vec!["a", "b"]);
    }

    #[test]
    fn multi_line_redraw_with_cursor_up() {
        // log-update style: draw two status lines, move up, erase down, redraw.
        let input = "header\nRunning 1/3\nfile a\n\x1b[2A\x1b[JRunning 3/3\nfile c\n";
        assert_eq!(s(input), vec!["header", "Running 3/3", "file c"]);
    }

    #[test]
    fn cursor_up_cannot_escape_the_screen() {
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&format!("line {i}\n"));
        }
        input.push_str("\x1b[999A\x1b[Jstatus\n");
        let out = s(&input);
        assert!(out.contains(&"line 0".to_string()));
        assert!(out.contains(&"line 149".to_string()));
        assert!(out.contains(&"status".to_string()));
    }

    #[test]
    fn osc_hyperlinks_keep_their_text() {
        let input = "see \x1b]8;;https://x.dev\x07docs\x1b]8;;\x07 now\n";
        assert_eq!(s(input), vec!["see docs now"]);
    }

    #[test]
    fn jest_tty_header() {
        let input = "\x1b[1G\x1b[0K\x1b[1m\x1b[2mDetermining test suites to run...\x1b[22m\x1b[22m\x1b[999D\x1b[K\x1b[999D\x1b[K\x1b[0m\x1b[7m\x1b[1m\x1b[31m FAIL \x1b[39m\x1b[22m\x1b[27m\x1b[0m \x1b[2msrc/\x1b[22m\x1b[1mauth.test.js\x1b[22m\r\n";
        assert_eq!(s(input), vec![" FAIL  src/auth.test.js"]);
    }

    #[test]
    fn backspace_overwrites() {
        assert_eq!(s("ab\x08c\n"), vec!["ac"]);
    }

    #[test]
    fn trailing_blank_lines_dropped_and_lines_right_trimmed() {
        assert_eq!(s("a   \n\n\n"), vec!["a"]);
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        let out = sanitize(b"ok \xff\xfe bytes\n");
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("ok "));
    }
}
