//! Fallback for unknown tools: keep error lines, file:line references and
//! their surroundings, condense stack traces, drop the rest.

use std::sync::LazyLock;

use regex::Regex;

use super::Ctx;
use crate::context::is_library_path;
use crate::report::{Engine, Loc, Report};

static ERR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:errors?|fatal|exception|panic(?:ked)?|traceback|fail(?:ed|ure|ures|s)?|",
        r"assert(?:ion)?(?:error)?|segmentation fault|core dumped|undefined reference|",
        r"cannot|can't|could not|couldn't|unable to|not found|no such file|",
        r"permission denied|refused|timed out|abort(?:ed)?|critical|uncaught|unhandled)\b",
        r"|ERR!|✗|✘|×"
    ))
    .unwrap()
});
static NEG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:0|no|zero|without) (?:errors?|failures?|failed|problems?|issues?)\b",
        r"|\b(?:errors?|failures?|failed):? 0\b"
    ))
    .unwrap()
});
static WARN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bwarn(?:ing)?s?\b|\bdeprecat").unwrap());
pub(crate) static LOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?:^|[\s(\[{'"`=<])((?:[A-Za-z]:)?(?:\.{1,2}/|/)?[\w@~.$+-]+(?:[/\\][\w@~.$+-]+)*\.[A-Za-z]\w{0,9})"#,
        r#"(?::(\d+)(?::(\d+))?|\((\d+)(?:,\s?(\d+))?\)|", line (\d+))"#
    ))
    .unwrap()
});
static PY_FRAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s+File "(.+?)", line (\d+)"#).unwrap());
static NODE_LOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(?((?:[A-Za-z]:)?[^()\s]+?):(\d+):(\d+)\)?$").unwrap());
static NODE_FRAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s+at \S").unwrap());

pub fn parse(lines: &[String], ctx: &Ctx) -> Report {
    let mut report = Report::new(Engine::Generic);
    let lines = collapse_duplicates(lines);
    if lines.len() <= ctx.small {
        report.passthrough = true;
        report.body = lines;
        return report;
    }
    let Condensed {
        lines,
        forced,
        trace,
        primary,
    } = condense_traces(lines);
    let success = ctx.exit_code == Some(0);

    let mut important: Vec<bool> = lines
        .iter()
        .zip(&forced)
        .map(|(l, &f)| f || (!success && is_error_line(l)))
        .collect();
    if success || !important.iter().any(|&b| b) {
        // Nothing failed: warnings are the most useful thing to surface.
        let mut budget = 15;
        for (i, l) in lines.iter().enumerate() {
            if budget > 0 && WARN.is_match(l) && !NEG.is_match(l) {
                important[i] = true;
                budget -= 1;
            }
        }
    }
    let ctx_lines = if success { 0 } else { ctx.context_lines };

    let mut keep = vec![false; lines.len()];
    for (i, _) in important.iter().enumerate().filter(|(_, &b)| b) {
        let lo = i.saturating_sub(ctx_lines);
        let hi = (i + ctx_lines).min(lines.len() - 1);
        keep[lo..=hi].iter_mut().for_each(|k| *k = true);
    }
    // The tail usually holds the summary / final status.
    let tail = if important.iter().any(|&b| b) { 5 } else { 15 };
    let n = lines.len();
    keep[n.saturating_sub(tail)..]
        .iter_mut()
        .for_each(|k| *k = true);

    let mut body = Vec::new();
    let mut i = 0;
    while i < n {
        if !keep[i] {
            let start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            body.push(format!("… ({} lines omitted)", i - start));
            continue;
        }
        let start = i;
        while i < n && keep[i] {
            i += 1;
        }
        emit_range(&lines[start..i], &important[start..i], &mut body);
    }
    report.body = cap_middle(body, ctx.max_lines);
    report.failed = ctx.exit_code.is_some_and(|c| c != 0);

    // Where to show code: innermost frames of traces, then locations on
    // error lines, in output order.
    if !success {
        let mut hints: Vec<(usize, Loc)> = primary;
        for (i, l) in lines.iter().enumerate() {
            if important[i] && !trace[i] {
                hints.extend(
                    extract_locs(std::slice::from_ref(l))
                        .into_iter()
                        .map(|loc| (i, loc)),
                );
            }
        }
        hints.sort_by_key(|(i, _)| *i);
        report.hint_locs = hints.into_iter().map(|(_, loc)| loc).collect();
    }
    report
}

fn is_error_line(l: &str) -> bool {
    (ERR.is_match(l) && !NEG.is_match(l)) || LOC.is_match(l)
}

/// Emits kept lines, folding runs of same-shaped noise ("Downloading 12%",
/// "Downloading 13%", ...) that are not themselves important.
fn emit_range(lines: &[String], important: &[bool], out: &mut Vec<String>) {
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        if !important[i] {
            let shape = shape_of(&lines[i]);
            while j < lines.len() && !important[j] && shape_of(&lines[j]) == shape {
                j += 1;
            }
        }
        if j - i >= 4 {
            out.push(lines[i].clone());
            out.push(format!("… ({} similar lines)", j - i - 2));
            out.push(lines[j - 1].clone());
        } else {
            out.extend(lines[i..j].iter().cloned());
        }
        i = j;
    }
}

fn shape_of(l: &str) -> String {
    l.chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

fn collapse_duplicates(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        if j - i > 2 && !lines[i].trim().is_empty() {
            out.push(format!("{}  (×{})", lines[i], j - i));
        } else {
            out.extend(lines[i..j].iter().cloned());
        }
        i = j;
    }
    out
}

/// Lines after stack-trace condensing.
struct Condensed {
    lines: Vec<String>,
    /// Traceback lines are kept whole.
    forced: Vec<bool>,
    /// Line is part of a stack trace (its locations are not error sites).
    trace: Vec<bool>,
    /// Innermost user frame of each trace, by line index.
    primary: Vec<(usize, Loc)>,
}

impl Condensed {
    fn push(&mut self, line: String, forced: bool, trace: bool) {
        self.lines.push(line);
        self.forced.push(forced);
        self.trace.push(trace);
    }
}

/// Drops library frames from Python tracebacks and Node stacks, keeps
/// tracebacks whole, and remembers where each trace's error was raised.
fn condense_traces(lines: Vec<String>) -> Condensed {
    let mut out = Condensed {
        lines: Vec::with_capacity(lines.len()),
        forced: Vec::with_capacity(lines.len()),
        trace: Vec::with_capacity(lines.len()),
        primary: Vec::new(),
    };
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if line
            .trim_start()
            .starts_with("Traceback (most recent call last)")
        {
            let indent = line.len() - line.trim_start().len();
            out.push(line.clone(), true, true);
            i += 1;
            let mut hidden = 0;
            let mut innermost: Option<Loc> = None;
            let mut frame_indent = indent + 2;
            while i < lines.len() {
                let l = &lines[i];
                let ind = l.len() - l.trim_start().len();
                if ind <= indent && !l.trim().is_empty() {
                    if hidden > 0 {
                        out.push(
                            format!("{}… {hidden} library frame(s)", " ".repeat(frame_indent)),
                            true,
                            true,
                        );
                        hidden = 0;
                    }
                    out.push(l.clone(), true, false); // the exception line
                    if let Some(loc) = innermost.take() {
                        out.primary.push((out.lines.len() - 1, loc));
                    }
                    i += 1;
                    break;
                }
                if let Some(c) = PY_FRAME.captures(l) {
                    frame_indent = ind;
                    if is_library_path(&c[1]) {
                        hidden += 1;
                        i += 1;
                        while i < lines.len()
                            && !PY_FRAME.is_match(&lines[i])
                            && lines[i].len() - lines[i].trim_start().len() > ind
                        {
                            i += 1;
                        }
                        continue;
                    }
                    if hidden > 0 {
                        out.push(
                            format!("{}… {hidden} library frame(s)", " ".repeat(ind)),
                            true,
                            true,
                        );
                        hidden = 0;
                    }
                    innermost = Some(Loc {
                        file: c[1].to_string(),
                        line: c[2].parse().unwrap_or(0),
                        col: None,
                    });
                }
                out.push(l.clone(), true, true);
                i += 1;
            }
            if hidden > 0 {
                out.push(
                    format!("{}… {hidden} library frame(s)", " ".repeat(frame_indent)),
                    true,
                    true,
                );
            }
            continue;
        }
        if NODE_FRAME.is_match(line) {
            let mut shown = 0;
            let mut hidden = 0;
            while i < lines.len() && NODE_FRAME.is_match(&lines[i]) {
                let l = &lines[i];
                if shown < 4 && !is_library_path(l) {
                    if shown == 0 {
                        if let Some(c) = NODE_LOC.captures(l) {
                            out.primary.push((
                                out.lines.len(),
                                Loc {
                                    file: c[1].to_string(),
                                    line: c[2].parse().unwrap_or(0),
                                    col: c[3].parse().ok(),
                                },
                            ));
                        }
                    }
                    out.push(l.clone(), false, true);
                    shown += 1;
                } else {
                    hidden += 1;
                }
                i += 1;
            }
            if hidden > 0 {
                out.push(format!("    … {hidden} more frame(s)"), false, true);
            }
            continue;
        }
        out.push(line.clone(), false, false);
        i += 1;
    }
    out
}

fn cap_middle(body: Vec<String>, max: usize) -> Vec<String> {
    if body.len() <= max || max < 10 {
        return body;
    }
    let head = max * 2 / 3;
    let tail = max - head - 1;
    let mut out: Vec<String> = body[..head].to_vec();
    out.push(format!(
        "… ({} more lines; see full log)",
        body.len() - head - tail
    ));
    out.extend(body[body.len() - tail..].iter().cloned());
    out
}

/// Project file locations mentioned in the given lines, in order.
pub fn extract_locs(lines: &[String]) -> Vec<Loc> {
    let mut out = Vec::new();
    for l in lines {
        for c in LOC.captures_iter(l) {
            let file = c[1].to_string();
            if is_library_path(&file) || file.starts_with("http") {
                continue;
            }
            let line = [2, 4, 6]
                .iter()
                .find_map(|&g| c.get(g).and_then(|m| m.as_str().parse().ok()));
            let col = [3, 5]
                .iter()
                .find_map(|&g| c.get(g).and_then(|m| m.as_str().parse().ok()));
            if let Some(line) = line {
                out.push(Loc { file, line, col });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(exit: Option<i32>) -> Ctx {
        Ctx {
            exit_code: exit,
            context_lines: 2,
            small: 5,
            max_lines: 120,
        }
    }

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn small_output_passes_through() {
        let r = parse(&lines("a\nb\nerror: x"), &ctx(Some(1)));
        assert!(r.passthrough);
        assert_eq!(r.body.len(), 3);
    }

    #[test]
    fn keeps_errors_with_context_and_tail() {
        let mut input: Vec<String> = (0..100).map(|i| format!("compiling unit {i}")).collect();
        input[50] = "src/main.c:12:5: error: expected ';' before 'return'".into();
        let r = parse(&input, &ctx(Some(1)));
        let body = r.body.join("\n");
        assert!(body.contains("src/main.c:12:5: error"));
        assert!(body.contains("compiling unit 48"));
        assert!(body.contains("compiling unit 52"));
        assert!(!body.contains("compiling unit 47\n"));
        assert!(body.contains("compiling unit 99"));
        assert!(body.contains("lines omitted"));
    }

    #[test]
    fn negated_error_counts_are_not_errors() {
        assert!(!is_error_line("Found 0 errors. Watching for file changes."));
        assert!(!is_error_line("no failures"));
        assert!(is_error_line("Found 3 errors."));
    }

    #[test]
    fn python_traceback_drops_library_frames() {
        let input = lines(concat!(
            "starting\n",
            "Traceback (most recent call last):\n",
            "  File \"/app/main.py\", line 3, in <module>\n",
            "    run()\n",
            "  File \"/usr/lib/python3.11/site-packages/x/core.py\", line 99, in run\n",
            "    inner()\n",
            "  File \"/app/lib.py\", line 7, in inner\n",
            "    1 / 0\n",
            "ZeroDivisionError: division by zero\n",
        ));
        let c = condense_traces(input);
        assert_eq!(c.lines.len(), 8);
        assert!(c.lines.iter().any(|l| l.contains("1 library frame")));
        assert!(!c.lines.iter().any(|l| l.contains("site-packages")));
        assert_eq!(
            c.lines.last().unwrap(),
            "ZeroDivisionError: division by zero"
        );
        assert!(c.forced[1..].iter().all(|&f| f));
        assert_eq!(c.primary.len(), 1);
        assert_eq!(c.primary[0].1.to_string(), "/app/lib.py:7");
    }

    #[test]
    fn trailing_library_frames_are_noted_before_the_exception() {
        let input = lines(concat!(
            "Traceback (most recent call last):\n",
            "  File \"app.py\", line 4, in load\n",
            "    return json.load(f)\n",
            "  File \"/usr/lib/python3.11/json/__init__.py\", line 293, in load\n",
            "    return loads(fp.read())\n",
            "json.decoder.JSONDecodeError: Expecting value\n",
        ));
        let c = condense_traces(input);
        assert_eq!(c.lines[3], "  … 1 library frame(s)");
        assert_eq!(c.lines[4], "json.decoder.JSONDecodeError: Expecting value");
    }

    #[test]
    fn extracts_locations_in_many_styles() {
        let locs = extract_locs(&lines(concat!(
            "src/a.ts(12,5): error TS2322: nope\n",
            "  File \"app/models.py\", line 40, in save\n",
            "./pkg/x.go:7:2: undefined: y\n",
            "see https://example.com/docs\n",
        )));
        let s: Vec<String> = locs.iter().map(|l| l.to_string()).collect();
        assert_eq!(
            s,
            vec!["src/a.ts:12:5", "app/models.py:40", "./pkg/x.go:7:2"]
        );
    }
}
