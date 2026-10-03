//! cargo / rustc: compiler diagnostics, libtest failures, panics and
//! backtraces.

use std::sync::LazyLock;

use regex::Regex;

use crate::context::is_library_path;
use crate::report::{cap, tidy, Code, Engine, Failure, Frame, Loc, Report};

static DIAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(error|warning)(?:\[(\w+)\])?: (.+)$").unwrap());
static ARROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*--> ((?:[A-Za-z]:)?[^:]+):(\d+):(\d+)$").unwrap());
static STDOUT_HDR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---- (.+?) std(?:out|err) ----$").unwrap());
static RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored").unwrap()
});
static PANIC_NEW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^thread '.*?'(?: \(\d+\))? panicked at ((?:[A-Za-z]:)?[^:\s]+):(\d+):(\d+):$")
        .unwrap()
});
static PANIC_OLD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^thread '.*?' panicked at '(.*)', ((?:[A-Za-z]:)?[^:\s]+):(\d+):(\d+)$").unwrap()
});
static BT_FUNC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\d+: (.+)$").unwrap());
static BT_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+at ((?:[A-Za-z]:)?[^:]+):(\d+):(\d+)$").unwrap());

/// Diagnostics that only summarise other diagnostics.
fn is_summary_diag(msg: &str) -> bool {
    msg.starts_with("could not compile")
        || msg.starts_with("aborting due to")
        || msg.starts_with("test failed, to rerun")
        || msg.starts_with("build failed")
        || msg.contains(" generated ") && msg.contains(" warning")
        || msg.starts_with("Some errors have detailed explanations")
        || msg.starts_with("For more information about")
}

pub fn parse(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Cargo);
    let mut warnings: Vec<Vec<String>> = Vec::new();
    let (mut passed, mut failed, mut ignored) = (0usize, 0usize, 0usize);
    let mut saw_results = false;
    let mut compile_errors = 0usize;

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if let Some(c) = DIAG.captures(line) {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().is_empty() && !DIAG.is_match(&lines[j]) {
                j += 1;
            }
            let block = &lines[i + 1..j];
            let msg = c[3].to_string();
            if !is_summary_diag(&msg) {
                if &c[1] == "error" {
                    compile_errors += 1;
                    let location = block.iter().find_map(|l| {
                        ARROW.captures(l).map(|a| Loc {
                            file: a[1].to_string(),
                            line: a[2].parse().unwrap_or(0),
                            col: a[3].parse().ok(),
                        })
                    });
                    let title = match c.get(2) {
                        Some(code) => format!("error[{}]: {msg}", code.as_str()),
                        None => format!("error: {msg}"),
                    };
                    let body: Vec<String> = block
                        .iter()
                        .filter(|l| !ARROW.is_match(l))
                        .cloned()
                        .collect();
                    report.failures.push(Failure {
                        title,
                        location,
                        code: (!body.is_empty()).then(|| Code {
                            lang: None,
                            lines: cap(body, 30),
                        }),
                        ..Default::default()
                    });
                } else {
                    let mut w = vec![line.clone()];
                    w.extend(block.iter().cloned());
                    warnings.push(w);
                }
            }
            i = j;
            continue;
        }
        if let Some(c) = STDOUT_HDR.captures(line) {
            let mut j = i + 1;
            while j < lines.len()
                && !STDOUT_HDR.is_match(&lines[j])
                && lines[j] != "failures:"
                && !RESULT.is_match(&lines[j])
            {
                j += 1;
            }
            report.failures.push(test_failure(&c[1], &lines[i + 1..j]));
            i = j;
            continue;
        }
        if let Some(c) = RESULT.captures(line) {
            saw_results = true;
            passed += c[1].parse::<usize>().unwrap_or(0);
            failed += c[2].parse::<usize>().unwrap_or(0);
            ignored += c[3].parse::<usize>().unwrap_or(0);
        }
        i += 1;
    }

    let mut parts = Vec::new();
    if compile_errors > 0 {
        parts.push(format!("build failed with {compile_errors} error(s)"));
    }
    if saw_results {
        let mut s = if failed > 0 {
            format!("{failed} failed, {passed} passed")
        } else {
            format!("{passed} passed")
        };
        if ignored > 0 {
            s.push_str(&format!(", {ignored} ignored"));
        }
        parts.push(s);
    }
    if !warnings.is_empty() {
        parts.push(format!("{} warning(s)", warnings.len()));
    }
    if !parts.is_empty() {
        report.summary = Some(parts.join("; "));
    }
    report.failed = !report.failures.is_empty() || failed > 0;

    // Warnings are noise next to errors, but they are the whole point of a
    // clean `cargo clippy` run.
    if !report.failed && !warnings.is_empty() {
        let shown = warnings.len().min(5);
        for w in warnings.iter().take(shown) {
            report.body.extend(w.iter().cloned());
            report.body.push(String::new());
        }
        report.body.pop();
        if warnings.len() > shown {
            report.notes.push(format!(
                "{} more warning(s) omitted",
                warnings.len() - shown
            ));
        }
    } else if !warnings.is_empty() {
        report
            .notes
            .push(format!("{} warning(s) omitted", warnings.len()));
    }
    report
}

fn test_failure(name: &str, block: &[String]) -> Failure {
    let mut message = Vec::new();
    let mut frames = Vec::new();
    let mut location = None;
    let mut func: Option<String> = None;
    let mut in_bt = false;
    for line in block {
        if let Some(c) = PANIC_NEW.captures(line) {
            location = Some(loc(&c[1], &c[2], &c[3]));
            continue;
        }
        if let Some(c) = PANIC_OLD.captures(line) {
            location = Some(loc(&c[2], &c[3], &c[4]));
            message.push(c[1].to_string());
            continue;
        }
        if line == "stack backtrace:" {
            in_bt = true;
            continue;
        }
        if line.starts_with("note: run with `RUST_BACKTRACE")
            || line.starts_with("note: Some details are omitted")
        {
            continue;
        }
        if in_bt {
            if let Some(c) = BT_AT.captures(line) {
                if let Some(f) = func.take() {
                    let file = c[1].trim_start_matches("./").to_string();
                    let noise = f.contains("{{closure}}")
                        || f.contains("{closure#")
                        || f.contains("FnOnce")
                        || f.starts_with("core::")
                        || f.starts_with("std::")
                        || f.starts_with("__rust");
                    if !noise && !is_library_path(&file) {
                        frames.push(Frame {
                            loc: loc(&file, &c[2], &c[3]),
                            func: Some(f),
                            src: None,
                        });
                    }
                }
                continue;
            }
            if let Some(c) = BT_FUNC.captures(line) {
                func = Some(c[1].to_string());
                continue;
            }
        }
        message.push(line.clone());
    }
    frames.truncate(4);
    frames.reverse();
    if location.is_none() {
        location = frames.last().map(|f| f.loc.clone());
    }
    Failure {
        title: name.to_string(),
        location,
        message: cap(tidy(message), 30),
        frames,
        ..Default::default()
    }
}

fn loc(file: &str, line: &str, col: &str) -> Loc {
    Loc {
        file: file.trim_start_matches("./").to_string(),
        line: line.parse().unwrap_or(0),
        col: col.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize::sanitize;

    fn fixture(name: &str) -> Vec<String> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        sanitize(&std::fs::read(path).unwrap())
    }

    #[test]
    fn test_failures_with_backtrace() {
        let r = parse(&fixture("cargo_test.log"));
        assert_eq!(r.failures.len(), 3);
        // cargo stops after the first failing test binary, so tests/bulk.rs never ran.
        assert_eq!(r.summary.as_deref(), Some("3 failed, 1 passed"));
        let f = &r.failures[1];
        assert_eq!(f.title, "tests::parses_port");
        assert_eq!(f.location.as_ref().unwrap().to_string(), "src/lib.rs:2:22");
        assert_eq!(
            f.message,
            vec![
                "called `Result::unwrap()` on an `Err` value: ParseIntError { kind: InvalidDigit }"
            ]
        );
        let funcs: Vec<&str> = f.frames.iter().filter_map(|f| f.func.as_deref()).collect();
        assert_eq!(funcs, vec!["rs::tests::parses_port", "rs::parse_port"]);
    }

    #[test]
    fn without_backtrace_same_failures() {
        let a = parse(&fixture("cargo_test.log"));
        let b = parse(&fixture("cargo_test_nobt.log"));
        for (x, y) in a.failures.iter().zip(&b.failures) {
            assert_eq!(x.title, y.title);
            assert_eq!(x.location, y.location);
            assert_eq!(x.message, y.message);
        }
    }

    #[test]
    fn compile_errors() {
        let r = parse(&fixture("cargo_compile_err.log"));
        assert_eq!(r.failures.len(), 2);
        assert_eq!(r.failures[0].title, "error[E0308]: mismatched types");
        assert_eq!(
            r.failures[0].location.as_ref().unwrap().to_string(),
            "src/lib.rs:38:5"
        );
        assert!(r.failures[1]
            .code
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|l| l.contains("help: consider cloning")));
        assert_eq!(r.summary.as_deref(), Some("build failed with 2 error(s)"));
    }

    #[test]
    fn warnings_shown_when_build_is_clean() {
        let r = parse(&fixture("cargo_build_tty.log"));
        assert!(!r.failed);
        assert!(r.body.iter().any(|l| l.contains("unused variable")));
    }
}
