//! Python unittest and the runners built on it (Django's `manage.py test`
//! and `runtests.py`, `python -m unittest`): `FAIL:` / `ERROR:` blocks
//! between `=====` rules, the traceback inside each, and the `Ran N tests`
//! plus `FAILED (failures=2, errors=1)` summary.

use std::sync::LazyLock;

use regex::Regex;

use crate::context::is_library_path;
use crate::report::{cap, tidy, Engine, Failure, Frame, Loc, Report};

static RULE_EQ: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^={20,}$"));
static RULE_DASH: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^-{20,}$"));
static HEADER: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^(FAIL|ERROR|UNEXPECTED SUCCESS): (.+)$"));
/// `test_x (pkg.mod.Class.test_x)` on 3.11+, `test_x (pkg.mod.Class)` before,
/// optionally followed by a subtest's parameters: ` (qty=3)` or ` [label]`.
static TEST_ID: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^(\w+) \(([\w.]+)\)(.*)$"));
static RAN: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^Ran (\d+) tests? in ([0-9.]+)s$"));
static RESULT: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^(OK|FAILED)(?: \((.*)\))?$"));
static FRAME: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r#"^\s+File "(.+?)", line (\d+)(?:, in (.+))?$"#));
static CHAIN: LazyLock<Regex> = LazyLock::new(|| {
    crate::re::re(
        r"^(?:During handling of the above exception|The above exception was the direct cause)",
    )
});

pub fn parse(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Unittest);
    let mut ran: Option<(String, String)> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if RULE_EQ.is_match(line) {
            if let Some(h) = lines.get(i + 1).and_then(|l| HEADER.captures(l)) {
                let kind = h[1].to_string();
                let title = test_title(&h[2]);
                // Verbose runs repeat the docstring under the header; skip
                // to the dashed rule that opens the traceback.
                let mut j = i + 2;
                while j < lines.len() && j < i + 6 && !RULE_DASH.is_match(&lines[j]) {
                    j += 1;
                }
                let start = if j < lines.len() && RULE_DASH.is_match(&lines[j]) {
                    j + 1
                } else {
                    i + 2
                };
                let mut end = start;
                while end < lines.len() && !block_ends(lines, end) {
                    end += 1;
                }
                report
                    .failures
                    .push(failure(&kind, title, &lines[start..end]));
                i = end;
                continue;
            }
        }
        if let Some(c) = RAN.captures(line) {
            ran = Some((c[1].to_string(), c[2].to_string()));
        } else if let Some(c) = RESULT.captures(line) {
            report.failed = &c[1] == "FAILED";
            report.summary = Some(summary(
                &c[1],
                c.get(2).map_or("", |m| m.as_str()),
                ran.as_ref(),
            ));
        }
        i += 1;
    }
    if !report.failures.is_empty() {
        report.failed = true;
    }
    report
}

fn block_ends(lines: &[String], i: usize) -> bool {
    let next = lines.get(i + 1).map(String::as_str).unwrap_or("");
    (RULE_EQ.is_match(&lines[i]) && HEADER.is_match(next))
        || (RULE_DASH.is_match(&lines[i]) && RAN.is_match(next))
}

/// `test_x (pkg.mod.Class.test_x) (qty=3)` -> `pkg.mod.Class.test_x (qty=3)`,
/// the id you pass back to the runner to re-run just that test.
fn test_title(header: &str) -> String {
    let Some(c) = TEST_ID.captures(header) else {
        return header.trim().to_string();
    };
    let (name, path, rest) = (&c[1], &c[2], &c[3]);
    let id = if path.ends_with(&format!(".{name}")) {
        path.to_string()
    } else {
        format!("{path}.{name}")
    };
    format!("{id}{rest}")
}

fn failure(kind: &str, title: String, block: &[String]) -> Failure {
    // Only the last traceback of a chain says where the error surfaced;
    // the earlier exception lines are kept as context.
    let mut chained: Vec<String> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut message: Vec<String> = Vec::new();
    let mut in_trace = false;
    let mut k = 0;
    while k < block.len() {
        let line = &block[k];
        if line.starts_with("Traceback (most recent call last)") {
            if !message.is_empty() {
                chained.extend(tidy(std::mem::take(&mut message)));
            }
            frames.clear();
            in_trace = true;
            k += 1;
            continue;
        }
        if CHAIN.is_match(line) {
            k += 1;
            continue;
        }
        if in_trace {
            if let Some(c) = FRAME.captures(line) {
                let indent = line.len() - line.trim_start().len();
                let mut src = None;
                k += 1;
                // The source line and 3.11+ position carets are indented
                // deeper than the `File` line.
                while k < block.len()
                    && !block[k].trim().is_empty()
                    && block[k].len() - block[k].trim_start().len() > indent
                {
                    let t = block[k].trim();
                    if src.is_none() && !t.chars().all(|ch| matches!(ch, '^' | '~')) {
                        src = Some(t.to_string());
                    }
                    k += 1;
                }
                frames.push(Frame {
                    loc: Loc {
                        file: c[1].to_string(),
                        line: c[2].parse().unwrap_or(0),
                        col: None,
                    },
                    func: c.get(3).map(|m| m.as_str().to_string()),
                    src,
                });
                continue;
            }
            in_trace = false;
        }
        message.push(line.clone());
        k += 1;
    }

    let mut user: Vec<Frame> = frames
        .into_iter()
        .filter(|f| !is_library_path(&f.loc.file))
        .collect();
    if user.len() > 4 {
        user.drain(..user.len() - 4);
    }
    let title = match kind {
        "ERROR" => format!("{title} (error)"),
        "UNEXPECTED SUCCESS" => format!("{title} (unexpected success)"),
        _ => title,
    };
    let mut extra = Vec::new();
    let chained = cap(tidy(chained), 8);
    if !chained.is_empty() {
        extra.push(("while handling".to_string(), chained));
    }
    Failure {
        title,
        location: user.last().map(|f| f.loc.clone()),
        message: cap(tidy(message), 30),
        frames: user,
        code: None,
        extra,
        dir_hints: Vec::new(),
    }
}

/// `FAILED (failures=2, errors=1, skipped=1)` and `Ran 51 tests in 0.002s`
/// -> `2 failed, 1 error, 1 skipped (51 tests in 0.002s)`.
fn summary(result: &str, counts: &str, ran: Option<&(String, String)>) -> String {
    let mut parts = Vec::new();
    let mut not_passed = 0usize;
    for kv in counts.split(", ").filter(|s| !s.is_empty()) {
        let Some((key, n)) = kv.split_once('=') else {
            continue;
        };
        let n: usize = n.trim().parse().unwrap_or(0);
        not_passed += n;
        let label = match key.trim() {
            "failures" => "failed",
            "errors" if n == 1 => "error",
            "errors" => "errors",
            "skipped" => "skipped",
            "expected failures" => "xfailed",
            "unexpected successes" => "unexpected successes",
            other => other,
        };
        parts.push(format!("{n} {label}"));
    }
    match (result, ran) {
        ("OK", Some((n, t))) => {
            let total: usize = n.parse().unwrap_or(0);
            let mut s = format!("{} passed", total.saturating_sub(not_passed));
            for p in &parts {
                s.push_str(", ");
                s.push_str(p);
            }
            format!("{s} in {t}s")
        }
        (_, Some((n, t))) => format!("{} ({n} tests in {t}s)", parts.join(", ")),
        _ => parts.join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize::sanitize;

    fn fixture(name: &str) -> Report {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        parse(&sanitize(&std::fs::read(path).unwrap()))
    }

    #[test]
    fn verbose_run() {
        let r = fixture("unittest_verbose.log");
        assert!(r.failed);
        assert_eq!(
            r.summary.as_deref(),
            Some("3 failed, 1 error, 1 skipped (51 tests in 0.004s)")
        );
        let titles: Vec<&str> = r.failures.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "tests.test_stock.TakeTests.test_unknown_sku (error)",
                "tests.test_stock.ReorderTests.test_low_items",
                "tests.test_stock.ReorderTests.test_reorder_point_per_item",
                "tests.test_stock.TakeTests.test_take_in_steps (qty=3)",
            ]
        );
        // The KeyError surfaced in library code under test, not in the test.
        let err = &r.failures[0];
        assert_eq!(err.location.as_ref().unwrap().line, 22);
        assert!(err
            .location
            .as_ref()
            .unwrap()
            .file
            .ends_with("inventory/stock.py"));
        assert_eq!(
            err.frames[0].src.as_deref(),
            Some("self.assertEqual(self.stock.take(\"screw\", 1), 0)")
        );
        assert_eq!(err.message, ["KeyError: 'screw'"]);
        // assertEqual's diff is kept with the message.
        let low = &r.failures[1];
        assert_eq!(
            low.message[0],
            "AssertionError: Lists differ: ['washer'] != ['nut', 'washer']"
        );
        assert!(low.message.iter().any(|l| l == "+ ['nut', 'washer']"));
    }

    #[test]
    fn plain_run_matches_verbose() {
        let v = fixture("unittest_verbose.log");
        let p = fixture("unittest_plain.log");
        let titles = |r: &Report| {
            r.failures
                .iter()
                .map(|f| f.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(&v), titles(&p));
        assert!(p
            .summary
            .unwrap()
            .starts_with("3 failed, 1 error, 1 skipped (51 tests in "));
    }

    #[test]
    fn django_run() {
        let r = fixture("unittest_django.log");
        assert_eq!(
            r.summary.as_deref(),
            Some("3 failed, 21 skipped (652 tests in 0.888s)")
        );
        assert_eq!(r.failures.len(), 3);
        assert_eq!(
            r.failures[0].title,
            "utils_tests.test_text.TestUtilsText.test_slugify (value='__strip__underscore-value___')"
        );
        assert_eq!(r.failures[0].location.as_ref().unwrap().line, 370);
    }

    #[test]
    fn chained_and_old_style_ids() {
        let lines: Vec<String> = [
            "======================================================================",
            "ERROR: test_load (app.tests.LoaderTest)",
            "----------------------------------------------------------------------",
            "Traceback (most recent call last):",
            "  File \"/srv/app/loader.py\", line 8, in load",
            "    return json.loads(raw)",
            "ValueError: bad json",
            "",
            "During handling of the above exception, another exception occurred:",
            "",
            "Traceback (most recent call last):",
            "  File \"/usr/lib/python3.8/unittest/case.py\", line 60, in testPartExecutor",
            "    yield",
            "  File \"/srv/app/tests.py\", line 12, in test_load",
            "    load(\"{\")",
            "  File \"/srv/app/loader.py\", line 10, in load",
            "    raise LoadError(path) from None",
            "app.loader.LoadError: settings.json",
            "",
            "----------------------------------------------------------------------",
            "Ran 3 tests in 0.010s",
            "",
            "FAILED (errors=1)",
        ]
        .map(String::from)
        .to_vec();
        let r = parse(&lines);
        assert_eq!(r.summary.as_deref(), Some("1 error (3 tests in 0.010s)"));
        let f = &r.failures[0];
        assert_eq!(f.title, "app.tests.LoaderTest.test_load (error)");
        assert_eq!(f.message, ["app.loader.LoadError: settings.json"]);
        assert_eq!(f.frames.len(), 2, "stdlib frame dropped");
        assert_eq!(f.location.as_ref().unwrap().line, 10);
        assert_eq!(f.extra[0].1, ["ValueError: bad json"]);
    }

    #[test]
    fn ok_summary() {
        let lines: Vec<String> = ["Ran 652 tests in 0.888s", "", "OK (skipped=21)"]
            .map(String::from)
            .to_vec();
        let r = parse(&lines);
        assert!(!r.failed);
        assert_eq!(
            r.summary.as_deref(),
            Some("631 passed, 21 skipped in 0.888s")
        );
    }
}
