//! `go test` / `go build` / `go vet`: failing tests (plain and -v), panics
//! with goroutine traces, and build errors.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::context::is_library_path;
use crate::report::{cap, dedent, tidy, Engine, Failure, Frame, Loc, Report};

static RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^=== (RUN|PAUSE|CONT|NAME)\s+(\S+)").unwrap());
static RESULT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)--- (FAIL|PASS|SKIP): (\S+)(?: \([0-9.]+s\))?").unwrap());
static PKG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(ok|FAIL|\?)\s*\t(\S+)(?:[\t ](.*))?$").unwrap());
static BUILD_HDR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^# (\S+)").unwrap());
static GO_LOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:vet: )?((?:[A-Za-z]:)?[^\s:]+\.go):(\d+)(?::(\d+))?: ").unwrap()
});
static TRACE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+((?:[A-Za-z]:)?\S+\.go):(\d+)(?: \+0x[0-9a-f]+)?$").unwrap());

pub fn parse(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Go);
    let mut current: Option<String> = None;
    let mut buffers: HashMap<String, Vec<String>> = HashMap::new();
    let mut names: Vec<Option<String>> = Vec::new(); // test name per failure
    let mut pending: Vec<usize> = Vec::new(); // failures awaiting their package line
    let mut passed = 0usize;
    let mut pkgs_ok = 0usize;
    let mut pkgs_failed = 0usize;
    let mut last_was_bare_fail = false;

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if let Some(c) = RUN.captures(line) {
            if &c[1] != "PAUSE" {
                current = Some(c[2].to_string());
            }
            i += 1;
            continue;
        }
        if let Some(c) = RESULT.captures(line) {
            let indent = c[1].len();
            let name = c[3].to_string();
            current = None;
            if &c[2] != "FAIL" {
                if &c[2] == "PASS" {
                    passed += 1;
                }
                buffers.remove(&name);
                i += 1;
                continue;
            }
            let mut body = buffers.remove(&name).unwrap_or_default();
            let mut j = i + 1;
            while j < lines.len() {
                let l = &lines[j];
                let ind = l.len() - l.trim_start().len();
                if l.trim().is_empty() || ind <= indent || RESULT.is_match(l) || RUN.is_match(l) {
                    break;
                }
                body.push(l.clone());
                j += 1;
            }
            last_was_bare_fail = body.is_empty();
            report.failures.push(test_failure(&name, &body));
            names.push(Some(name));
            pending.push(report.failures.len() - 1);
            i = j;
            continue;
        }
        if line.starts_with("panic: ") || line.starts_with("fatal error: ") {
            let mut j = i + 1;
            while j < lines.len()
                && !PKG.is_match(&lines[j])
                && lines[j] != "FAIL"
                && !lines[j].starts_with("exit status ")
                && !RESULT.is_match(&lines[j])
                && !RUN.is_match(&lines[j])
            {
                j += 1;
            }
            let (message, frames) = parse_panic(&lines[i..j]);
            let target = match report.failures.last_mut() {
                Some(f) if last_was_bare_fail => f,
                _ => {
                    report.failures.push(Failure {
                        title: "panic".into(),
                        ..Default::default()
                    });
                    names.push(None);
                    pending.push(report.failures.len() - 1);
                    report.failures.last_mut().unwrap()
                }
            };
            target.message = message;
            target.location = frames.last().map(|f| f.loc.clone());
            target.frames = frames;
            last_was_bare_fail = false;
            i = j;
            continue;
        }
        if let Some(c) = BUILD_HDR.captures(line) {
            let pkg = c[1].to_string();
            let mut j = i + 1;
            let mut errs = Vec::new();
            while j < lines.len() {
                let l = &lines[j];
                if GO_LOC.is_match(l) || l.starts_with('\t') || l.starts_with("    ") {
                    errs.push(l.trim_start_matches("vet: ").to_string());
                    j += 1;
                } else if l.starts_with("# [") {
                    j += 1; // vet prints "# [pkg]" sub-headers
                } else {
                    break;
                }
            }
            if !errs.is_empty() {
                let location = errs.iter().find_map(|l| go_loc(l));
                report.failures.push(Failure {
                    title: format!("build failed: {pkg}"),
                    location,
                    message: cap(errs, 20),
                    dir_hints: vec![pkg],
                    ..Default::default()
                });
                names.push(None);
            }
            i = j;
            continue;
        }
        if let Some(c) = PKG.captures(line) {
            match &c[1] {
                "ok" => pkgs_ok += 1,
                "FAIL" => {
                    pkgs_failed += 1;
                    let pkg = c[2].to_string();
                    for idx in pending.drain(..) {
                        report.failures[idx].dir_hints.push(pkg.clone());
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        if let Some(name) = &current {
            if line.starts_with(' ') || line.starts_with('\t') {
                buffers.entry(name.clone()).or_default().push(line.clone());
            }
        }
        i += 1;
    }

    // A parent test whose only failure is a failing subtest adds nothing.
    let all: Vec<String> = names.iter().flatten().cloned().collect();
    let mut keep = Vec::new();
    for (f, name) in report.failures.drain(..).zip(names) {
        let redundant = f.message.is_empty()
            && name
                .as_ref()
                .is_some_and(|n| all.iter().any(|m| m.starts_with(&format!("{n}/"))));
        if !redundant {
            keep.push(f);
        }
    }
    report.failures = keep;

    let failed_tests = report
        .failures
        .iter()
        .filter(|f| !f.title.starts_with("build failed"))
        .count();
    let builds = report.failures.len() - failed_tests;
    let mut parts = Vec::new();
    if failed_tests > 0 {
        parts.push(format!("{failed_tests} failed"));
    }
    if builds > 0 {
        parts.push(format!("{builds} build error(s)"));
    }
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if pkgs_failed > 0 {
        parts.push(format!("packages: {pkgs_failed} failed, {pkgs_ok} ok"));
    } else if pkgs_ok > 0 {
        parts.push(format!("{pkgs_ok} package(s) ok"));
    }
    if !parts.is_empty() {
        report.summary = Some(parts.join(", "));
    }
    report.failed = !report.failures.is_empty() || pkgs_failed > 0;
    report
}

fn go_loc(line: &str) -> Option<Loc> {
    let c = GO_LOC.captures(line)?;
    Some(Loc {
        file: c[1].trim_start_matches("./").to_string(),
        line: c[2].parse().ok()?,
        col: c.get(3).and_then(|m| m.as_str().parse().ok()),
    })
}

fn test_failure(name: &str, body: &[String]) -> Failure {
    let message = cap(tidy(dedent(body)), 30);
    let location = message.iter().find_map(|l| go_loc(l));
    Failure {
        title: name.to_string(),
        location,
        message,
        ..Default::default()
    }
}

/// Splits a panic into its message and the user frames of the goroutine
/// trace (innermost last).
fn parse_panic(lines: &[String]) -> (Vec<String>, Vec<Frame>) {
    let mut message = Vec::new();
    let mut frames = Vec::new();
    let mut in_trace = false;
    let mut func: Option<String> = None;
    for line in lines {
        if line.starts_with("goroutine ") && line.ends_with(':') {
            in_trace = true;
            continue;
        }
        if !in_trace {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let t = match t.find(" [recovered") {
                Some(i) => t[..i].to_string(),
                None => t.to_string(),
            };
            if !message.contains(&t) {
                message.push(t);
            }
            continue;
        }
        if let Some(c) = TRACE_FILE.captures(line) {
            let file = c[1].to_string();
            if let Some(f) = func.take() {
                let internal = f.starts_with("testing.")
                    || f.starts_with("runtime.")
                    || f.starts_with("panic(")
                    || f.starts_with("created by ");
                if !internal && !is_library_path(&file) {
                    frames.push(Frame {
                        loc: Loc {
                            file,
                            line: c[2].parse().unwrap_or(0),
                            col: None,
                        },
                        func: Some(short_func(&f)),
                        src: None,
                    });
                }
            }
        } else if !line.trim().is_empty() {
            func = Some(line.trim().to_string());
        }
    }
    frames.truncate(4);
    frames.reverse();
    (message, frames)
}

/// "example.com/shop/auth.PrimaryRole(...)" -> "auth.PrimaryRole"
fn short_func(f: &str) -> String {
    let no_args = match f.rfind('(') {
        Some(i) if f.ends_with(')') => &f[..i],
        _ => f,
    };
    no_args.rsplit('/').next().unwrap_or(no_args).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize::sanitize;

    fn fixture(name: &str) -> Vec<String> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        sanitize(&std::fs::read(path).unwrap())
    }

    fn check(r: &Report) {
        let titles: Vec<&str> = r.failures.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["TestValidateExpired", "TestTable/stale", "TestPrimaryRole"]
        );
        assert_eq!(
            r.failures[0].message,
            vec!["auth_test.go:14: Validate() status = 401, want 200 (err=token expired)"]
        );
        assert_eq!(
            r.failures[0].location.as_ref().unwrap().to_string(),
            "auth_test.go:14"
        );
        assert_eq!(r.failures[0].dir_hints, vec!["example.com/shop/auth"]);
        let p = &r.failures[2];
        assert_eq!(
            p.message,
            vec!["panic: runtime error: index out of range [0] with length 0"]
        );
        assert_eq!(p.frames.len(), 2);
        assert_eq!(p.frames[1].func.as_deref(), Some("auth.PrimaryRole"));
        assert!(p.location.as_ref().unwrap().file.ends_with("auth/auth.go"));
    }

    #[test]
    fn plain() {
        let r = parse(&fixture("go_plain.log"));
        check(&r);
        assert_eq!(
            r.summary.as_deref(),
            Some("3 failed, packages: 1 failed, 1 ok")
        );
    }

    #[test]
    fn verbose() {
        let r = parse(&fixture("go_verbose.log"));
        check(&r);
        assert!(r.summary.as_deref().unwrap().contains("passed"));
    }

    #[test]
    fn build_errors() {
        let r = parse(&fixture("go_build.log"));
        assert_eq!(r.failures.len(), 2);
        assert_eq!(r.failures[0].title, "build failed: example.com/shop/util");
        assert_eq!(
            r.failures[0].location.as_ref().unwrap().to_string(),
            "util/util.go:4:25"
        );
        assert!(r.failed);
    }
}
