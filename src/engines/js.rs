//! Jest and Vitest. Both print a block per failing test with the assertion
//! message, an optional diff, a code frame and a stack trace.

use std::sync::LazyLock;

use regex::Regex;

use crate::context::{is_library_path, lang_for};
use crate::report::{cap, dedent, tidy, Code, Engine, Failure, Frame, Loc, Report};

static JEST_SUITE: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^\s*(PASS|FAIL)\s+(\S.*?)\s*(?:\([0-9.]+\s*m?s\))?$"));
static BULLET: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^(\s*)● (.+)$"));
static NODE_STACK: LazyLock<Regex> = LazyLock::new(|| {
    crate::re::re(r"^\s*at (?:(.+?) \()?((?:[A-Za-z]:)?[^():]+?):(\d+):(\d+)\)?$")
});
static JEST_CODE: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^\s*(?:>\s*)?\d+ \||^\s+\|(?:\s|$)"));
static CONSOLE: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^\s*console\.(?:log|warn|error|info|debug|trace)$"));

static VITEST_SEP: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^\s*⎯{2,}"));
static VITEST_FAIL: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^\s*FAIL\s+(.+?)\s*$"));
static VITEST_STACK: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^\s*❯ (?:(\S+) )?((?:[A-Za-z]:)?[^\s:]+?):(\d+):(\d+)$"));
static VITEST_CODE: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^\s*\d+\||^\s+\|(?:\s|$)"));

struct Block {
    title: String,
    suite: Option<String>,
    lines: Vec<String>,
}

pub fn parse_jest(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Jest);
    let mut suite: Option<String> = None;
    let mut blocks: Vec<Block> = Vec::new();
    let mut open = false;
    let mut tests_summary = None;
    let mut suites_summary = None;
    let mut failed_suites = 0usize;

    for line in lines {
        let t = line.trim();
        if let Some(c) = JEST_SUITE.captures(line) {
            open = false;
            if &c[1] == "FAIL" {
                failed_suites += 1;
            }
            suite = Some(c[2].to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("Tests:") {
            open = false;
            tests_summary = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("Test Suites:") {
            open = false;
            suites_summary = Some(rest.trim().to_string());
            continue;
        }
        if t.starts_with("Snapshots:")
            || t.starts_with("Time:")
            || t.starts_with("Ran all test suites")
            || t.starts_with("Summary of all failing tests")
            || CONSOLE.is_match(line)
        {
            open = false;
            continue;
        }
        if let Some(c) = BULLET.captures(line) {
            let title = c[2].trim().to_string();
            open = title != "Console";
            if open {
                blocks.push(Block {
                    title,
                    suite: suite.clone(),
                    lines: Vec::new(),
                });
            }
            continue;
        }
        if open {
            if let Some(b) = blocks.last_mut() {
                b.lines.push(line.clone());
            }
        }
    }

    for b in blocks {
        let f = jest_block(b);
        let dup = report
            .failures
            .iter()
            .any(|g| g.title == f.title && g.location == f.location);
        if !dup {
            report.failures.push(f);
        }
    }

    report.summary = match (tests_summary, suites_summary) {
        (Some(t), Some(s)) if s.contains("failed") && !t.contains("failed") => {
            Some(format!("{t} (suites: {s})"))
        }
        (Some(t), _) => Some(t),
        (None, Some(s)) => Some(format!("suites: {s}")),
        (None, None) => None,
    };
    report.failed = !report.failures.is_empty()
        || failed_suites > 0
        || report
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("failed"));
    report
}

fn jest_block(b: Block) -> Failure {
    let lines = dedent(&b.lines);
    let mut message = Vec::new();
    let mut code = Vec::new();
    let mut frames = Vec::new();
    for line in &lines {
        if JEST_CODE.is_match(line) {
            code.push(line.clone());
        } else if let Some(c) = NODE_STACK.captures(line) {
            let file = c[2].to_string();
            if !is_library_path(&file) && !file.contains("/jest-") && !file.contains("/@jest/") {
                frames.push(Frame {
                    loc: Loc {
                        file,
                        line: c[3].parse().unwrap_or(0),
                        col: c[4].parse().ok(),
                    },
                    func: c
                        .get(1)
                        .map(|m| m.as_str().to_string())
                        .filter(|f| !f.starts_with("Object.")),
                    src: None,
                });
            }
        } else if !line.trim_start().starts_with("at ") {
            message.push(line.clone());
        }
    }
    frames.truncate(3);
    let location = frames.first().map(|f| f.loc.clone());
    let code = trim_code_frame(dedent(&code), |l| l.trim_start().starts_with('>'));
    let lang = location.as_ref().and_then(|l| lang_for(&l.file));
    let title = match (&b.suite, b.title.as_str()) {
        (Some(s), "Test suite failed to run") => format!("{s} › Test suite failed to run"),
        _ => b.title,
    };
    Failure {
        title,
        location,
        message: cap(tidy(message), 40),
        frames,
        code: (!code.is_empty()).then_some(Code { lang, lines: code }),
        extra: Vec::new(),
        dir_hints: b.suite.into_iter().collect(),
    }
}

/// Keeps two lines either side of the marked line (plus the caret line).
fn trim_code_frame(code: Vec<String>, is_marker: impl Fn(&str) -> bool) -> Vec<String> {
    let Some(m) = code.iter().position(|l| is_marker(l)) else {
        return code;
    };
    let start = m.saturating_sub(2);
    let mut end = (m + 3).min(code.len());
    // The caret line does not count towards the two trailing lines.
    if code
        .get(m + 1)
        .is_some_and(|l| l.trim_start().starts_with('|'))
    {
        end = (m + 4).min(code.len());
    }
    code[start..end].to_vec()
}

pub fn parse_vitest(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Vitest);
    let mut in_failures = false;
    let mut blocks: Vec<Block> = Vec::new();
    let mut open = false;
    let mut files = None;
    let mut tests = None;

    for line in lines {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Test Files") {
            files = Some(rest.trim().to_string());
            open = false;
            in_failures = false;
            continue;
        }
        if t.starts_with("Tests ") && files.is_some() && tests.is_none() {
            tests = Some(t["Tests ".len()..].trim().to_string());
            continue;
        }
        if VITEST_SEP.is_match(line) {
            open = false;
            if t.contains("Failed Tests") || t.contains("Failed Suites") {
                in_failures = true;
            } else if t.contains("Unhandled") {
                in_failures = true;
                open = true;
                blocks.push(Block {
                    title: "Unhandled error".to_string(),
                    suite: None,
                    lines: Vec::new(),
                });
            }
            continue;
        }
        if !in_failures {
            continue;
        }
        if let Some(c) = VITEST_FAIL.captures(line) {
            let title = c[1].to_string();
            let suite = title
                .split(" > ")
                .next()
                .map(|s| s.split(" [").next().unwrap_or(s).trim().to_string());
            blocks.push(Block {
                title,
                suite,
                lines: Vec::new(),
            });
            open = true;
            continue;
        }
        if open {
            if let Some(b) = blocks.last_mut() {
                b.lines.push(line.clone());
            }
        }
    }

    for b in blocks {
        let f = vitest_block(b);
        if !report
            .failures
            .iter()
            .any(|g| g.title == f.title && g.location == f.location)
        {
            report.failures.push(f);
        }
    }
    report.summary = match (tests, files) {
        (Some(t), Some(f)) if f.contains("failed") && !t.contains("failed") => {
            Some(format!("{t} (files: {f})"))
        }
        (Some(t), _) => Some(t),
        (None, Some(f)) => Some(format!("files: {f}")),
        (None, None) => None,
    };
    report.failed = !report.failures.is_empty()
        || report
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("failed"));
    report
}

fn vitest_block(b: Block) -> Failure {
    let mut message = Vec::new();
    let mut code = Vec::new();
    let mut frames = Vec::new();
    let mut code_done = false;
    for line in &b.lines {
        if let Some(c) = VITEST_STACK.captures(line) {
            if !code.is_empty() {
                code_done = true;
            }
            let file = c[2].to_string();
            if !is_library_path(&file) {
                frames.push(Frame {
                    loc: Loc {
                        file,
                        line: c[3].parse().unwrap_or(0),
                        col: c[4].parse().ok(),
                    },
                    func: c.get(1).map(|m| m.as_str().to_string()),
                    src: None,
                });
            }
        } else if VITEST_CODE.is_match(line) {
            // Only the first code frame: it belongs to the innermost user frame.
            if !code_done {
                code.push(line.clone());
            }
        } else if !line.trim_start().starts_with("at ") {
            message.push(line.clone());
        }
    }
    frames.truncate(3);
    let location = frames.first().map(|f| f.loc.clone());
    let code = dedent(&code);
    // The failing line is the one right above the caret line.
    let code = match code.iter().position(|l| l.trim_start().starts_with('|')) {
        Some(caret) if caret > 0 => {
            let m = caret - 1;
            let start = m.saturating_sub(2);
            let end = (caret + 3).min(code.len());
            code[start..end].to_vec()
        }
        _ => code,
    };
    let lang = location.as_ref().and_then(|l| lang_for(&l.file));
    Failure {
        title: b.title,
        location,
        message: cap(tidy(dedent(&message)), 40),
        frames,
        code: (!code.is_empty()).then_some(Code { lang, lines: code }),
        extra: Vec::new(),
        dir_hints: b.suite.into_iter().collect(),
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
    fn jest_plain() {
        let r = parse_jest(&fixture("jest_plain.log"));
        assert_eq!(
            r.summary.as_deref(),
            Some("3 failed, 121 passed, 124 total")
        );
        assert_eq!(r.failures.len(), 3);
        let f = &r.failures[0];
        assert_eq!(f.title, "AuthService › rejects wrong password with 200");
        assert_eq!(
            f.location.as_ref().unwrap().to_string(),
            "src/auth.test.js:12:24"
        );
        assert!(f.message.contains(&"Expected: 200".to_string()));
        assert!(f.message.contains(&"Received: 401".to_string()));
        let code = &f.code.as_ref().unwrap().lines;
        assert_eq!(code.len(), 6, "{code:#?}");
        assert!(code[2].starts_with("> 12 |"));
        let f = &r.failures[1];
        assert_eq!(
            f.location.as_ref().unwrap().to_string(),
            "src/auth.js:10:11"
        );
        assert_eq!(f.frames.len(), 2);
        assert_eq!(f.frames[0].func.as_deref(), Some("validateToken"));
    }

    #[test]
    fn jest_tty_matches_plain() {
        let a = parse_jest(&fixture("jest_plain.log"));
        let b = parse_jest(&fixture("jest_tty.log"));
        assert_eq!(a.failures.len(), b.failures.len());
        for (x, y) in a.failures.iter().zip(&b.failures) {
            assert_eq!(x.title, y.title);
            assert_eq!(x.message, y.message);
            assert_eq!(x.location, y.location);
        }
    }

    #[test]
    fn vitest_plain() {
        let r = parse_vitest(&fixture("vitest_plain.log"));
        assert_eq!(r.summary.as_deref(), Some("3 failed | 101 passed (104)"));
        assert_eq!(r.failures.len(), 3);
        let f = &r.failures[1];
        assert_eq!(
            f.title,
            "src/auth.test.ts > AuthService > accepts expired token"
        );
        assert_eq!(f.message, vec!["Error: token expired at 1000"]);
        assert_eq!(f.location.as_ref().unwrap().to_string(), "src/auth.ts:5:11");
        assert_eq!(f.frames.len(), 2);
        let code = &f.code.as_ref().unwrap().lines;
        assert!(code
            .iter()
            .any(|l| l.contains("throw new UnauthorizedError")));
    }
}
