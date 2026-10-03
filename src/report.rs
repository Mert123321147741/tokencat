//! The engine-independent result model and its Markdown rendering.

use std::fmt::{self, Write as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Engine {
    Pytest,
    Jest,
    Vitest,
    Go,
    Cargo,
    Generic,
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Engine::Pytest => "pytest",
            Engine::Jest => "jest",
            Engine::Vitest => "vitest",
            Engine::Go => "go",
            Engine::Cargo => "cargo",
            Engine::Generic => "output",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loc {
    pub file: String,
    pub line: usize,
    pub col: Option<usize>,
}

impl fmt::Display for Loc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.col {
            Some(c) => write!(f, "{}:{}:{}", self.file, self.line, c),
            None => write!(f, "{}:{}", self.file, self.line),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub loc: Loc,
    pub func: Option<String>,
    /// The source line the tool printed for this frame, if any.
    pub src: Option<String>,
}

/// A code excerpt, either printed by the tool itself or read from disk.
#[derive(Debug, Clone, Default)]
pub struct Code {
    pub lang: Option<&'static str>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Failure {
    pub title: String,
    pub location: Option<Loc>,
    pub message: Vec<String>,
    pub frames: Vec<Frame>,
    pub code: Option<Code>,
    /// Labelled extra sections, e.g. captured stdout.
    pub extra: Vec<(String, Vec<String>)>,
    /// Directory hints for resolving bare file names (Go prints `x_test.go:12`).
    pub dir_hints: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub engine: Engine,
    pub failed: bool,
    pub summary: Option<String>,
    pub failures: Vec<Failure>,
    /// Free-form lines, used by the generic engine and for passthrough.
    pub body: Vec<String>,
    /// Code excerpts that are not tied to a structured failure (generic engine).
    pub snippets: Vec<(Loc, Code)>,
    /// Locations worth showing code for when there are no structured failures.
    pub hint_locs: Vec<Loc>,
    pub notes: Vec<String>,
    /// Print `body` as-is without a heading (small outputs).
    pub passthrough: bool,
}

impl Report {
    pub fn new(engine: Engine) -> Self {
        Report {
            engine,
            failed: false,
            summary: None,
            failures: Vec::new(),
            body: Vec::new(),
            snippets: Vec::new(),
            hint_locs: Vec::new(),
            notes: Vec::new(),
            passthrough: false,
        }
    }
}

pub struct RenderOpts {
    pub max_failures: usize,
    pub exit_code: Option<i32>,
}

pub fn render(report: &Report, opts: &RenderOpts) -> String {
    let mut out = String::new();
    if report.passthrough {
        for line in &report.body {
            out.push_str(line);
            out.push('\n');
        }
        for (loc, code) in &report.snippets {
            let _ = writeln!(out, "\n{loc}");
            render_code(&mut out, code);
        }
        return out;
    }

    let failed = report.failed || !report.failures.is_empty();
    let mark = if failed { "✗" } else { "✓" };
    let mut heading = match (&report.summary, report.engine) {
        (Some(s), Engine::Generic) => format!("## {mark} {s}"),
        (Some(s), e) => format!("## {mark} {e}: {s}"),
        (None, Engine::Generic) => match opts.exit_code {
            Some(0) => "## ✓ command succeeded".to_string(),
            Some(c) => format!("## ✗ command failed (exit {c})"),
            None => String::new(),
        },
        (None, e) if failed => {
            format!("## ✗ {e}: {} failure(s)", report.failures.len())
        }
        (None, e) => format!("## ✓ {e}: passed"),
    };
    if report.engine != Engine::Generic {
        if let Some(code) = opts.exit_code.filter(|&c| c != 0) {
            let _ = write!(heading, " (exit {code})");
        }
    }
    if !heading.is_empty() {
        out.push_str(&heading);
        out.push('\n');
    }

    let shown = report.failures.len().min(opts.max_failures);
    // Parametrized tests often fail on the same line: show that code once.
    let mut seen_code: Vec<(&Code, usize)> = Vec::new();
    for (i, f) in report.failures.iter().take(shown).enumerate() {
        out.push('\n');
        let same_as = f.code.as_ref().and_then(|c| {
            seen_code
                .iter()
                .find(|(d, _)| d.lines == c.lines)
                .map(|(_, n)| *n)
        });
        render_failure(&mut out, i + 1, f, same_as);
        if let (Some(c), None) = (&f.code, same_as) {
            seen_code.push((c, i + 1));
        }
    }
    if report.failures.len() > shown {
        let rest = &report.failures[shown..];
        let names: Vec<&str> = rest.iter().take(8).map(|f| f.title.as_str()).collect();
        let more = if rest.len() > names.len() {
            "; …"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "\n… and {} more failure(s): {}{more}",
            rest.len(),
            names.join("; ")
        );
    }

    if !report.body.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        for line in &report.body {
            out.push_str(line);
            out.push('\n');
        }
    }

    for (loc, code) in &report.snippets {
        let _ = writeln!(out, "\n{loc}");
        render_code(&mut out, code);
    }

    if !report.notes.is_empty() {
        out.push('\n');
        for n in &report.notes {
            let _ = writeln!(out, "({n})");
        }
    }
    out
}

fn render_failure(out: &mut String, n: usize, f: &Failure, code_same_as: Option<usize>) {
    let _ = writeln!(out, "### {n}. {}", f.title);
    let primary = f.location.as_ref();
    // Without code, the frame line for the primary location says more than a
    // bare "at file:line", so let it stand in for it.
    let frame_shows_primary = f.code.is_none()
        && primary.is_some_and(|p| f.frames.iter().any(|fr| same_place(p, &fr.loc)));
    if let Some(loc) = primary {
        let loc_s = loc.to_string();
        if !f.title.contains(&loc_s) && !frame_shows_primary {
            let _ = writeln!(out, "at {loc_s}");
        }
    }
    for line in &f.message {
        out.push_str(line);
        out.push('\n');
    }
    // Frames beyond the primary location, innermost last as tools print them.
    for fr in &f.frames {
        if f.code.is_some() && primary.is_some_and(|p| same_place(p, &fr.loc)) {
            continue;
        }
        let _ = match (&fr.func, &fr.src) {
            (Some(func), Some(src)) => writeln!(out, "  at {} in {func}: {}", fr.loc, src.trim()),
            (Some(func), None) => writeln!(out, "  at {} in {func}", fr.loc),
            (None, Some(src)) => writeln!(out, "  at {}: {}", fr.loc, src.trim()),
            (None, None) => writeln!(out, "  at {}", fr.loc),
        };
    }
    for (label, lines) in &f.extra {
        let _ = writeln!(out, "--- {label} ---");
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    match (&f.code, code_same_as) {
        (Some(_), Some(prev)) => {
            let _ = writeln!(out, "(code: same as #{prev})");
        }
        (Some(code), None) => render_code(out, code),
        _ => {}
    }
}

/// Same line of the same file, even if one path is absolute and the other
/// relative.
fn same_place(a: &Loc, b: &Loc) -> bool {
    a.line == b.line
        && (a.file == b.file
            || a.file.ends_with(&format!("/{}", b.file))
            || b.file.ends_with(&format!("/{}", a.file)))
}

fn render_code(out: &mut String, code: &Code) {
    if code.lines.is_empty() {
        return;
    }
    let fence = if code.lines.iter().any(|l| l.contains("```")) {
        "````"
    } else {
        "```"
    };
    let _ = writeln!(out, "{fence}{}", code.lang.unwrap_or(""));
    for line in &code.lines {
        out.push_str(line);
        out.push('\n');
    }
    let _ = writeln!(out, "{fence}");
}

/// Removes the common leading indentation from a block of lines.
pub fn dedent(lines: &[String]) -> Vec<String> {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l.get(indent..)
                    .unwrap_or_else(|| l.trim_start())
                    .to_string()
            }
        })
        .collect()
}

/// Trims leading/trailing blank lines and squeezes repeated blank lines.
pub fn tidy(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for l in lines {
        if l.trim().is_empty() {
            if out.last().is_none_or(|p| p.is_empty()) {
                continue;
            }
            out.push(String::new());
        } else {
            out.push(l);
        }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// Caps a block at `max` lines, noting how many were dropped.
pub fn cap(mut lines: Vec<String>, max: usize) -> Vec<String> {
    if lines.len() > max {
        let dropped = lines.len() - max;
        lines.truncate(max);
        lines.push(format!("… ({dropped} more lines)"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_keeps_relative_indentation() {
        let lines = vec!["    a".to_string(), "      b".to_string(), "".to_string()];
        assert_eq!(dedent(&lines), vec!["a", "  b", ""]);
    }

    #[test]
    fn tidy_squeezes_blank_lines() {
        let lines = ["", "a", "", "", "b", ""].map(String::from).to_vec();
        assert_eq!(tidy(lines), vec!["a", "", "b"]);
    }

    #[test]
    fn dedent_handles_wide_chars_in_indent_free_lines() {
        let lines = vec!["  ✓ ok".to_string(), "  ● bad".to_string()];
        assert_eq!(dedent(&lines), vec!["✓ ok", "● bad"]);
    }
}
