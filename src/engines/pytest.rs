//! pytest: FAILURES / ERRORS blocks in long, short and line traceback
//! styles, plus the short test summary.

use std::sync::LazyLock;

use regex::Regex;

use crate::context::is_library_path;
use crate::report::{cap, dedent, tidy, Engine, Failure, Frame, Loc, Report};

static SECTION: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^=+ (.+?) =+$"));
static BLOCK: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^_{3,} (.+?) _{3,}$"));
static FRAME_SEP: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^(?:_ ){3,}_?$"));
static CAPTURED: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^-{3,} (.+?) -{3,}$"));
static LOC: LazyLock<Regex> = LazyLock::new(|| {
    crate::re::re(r"^((?:[A-Za-z]:)?[^\s:][^:]*?\.\w+):(\d+):(?: in (\S+)$| (.*)$|$)")
});
static SHORT: LazyLock<Regex> =
    LazyLock::new(|| crate::re::re(r"^(FAILED|ERROR|XPASS\S*) (\S+)(?: - (.*))?$"));
static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
    crate::re::re(
        r"^(?:\d+ (?:passed|failed|errors?|skipped|xfailed|xpassed|deselected|warnings?|rerun)\b.*|no tests ran.*) in [0-9.]+s.*$",
    )
});
static DEF: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^\s*(?:async\s+)?def\s+(\w+)"));
static LOCALS: LazyLock<Regex> = LazyLock::new(|| crate::re::re(r"^[A-Za-z_]\w* = "));

#[derive(PartialEq, Clone, Copy)]
enum Section {
    None,
    Failures,
    Errors,
    Short,
    Other,
}

struct Block {
    title: String,
    lines: Vec<String>,
    error: bool,
}

struct ShortEntry {
    id: String,
    msg: Option<String>,
    error: bool,
    used: bool,
}

pub fn parse(lines: &[String]) -> Report {
    let mut report = Report::new(Engine::Pytest);
    let mut section = Section::None;
    let mut blocks: Vec<Block> = Vec::new();
    let mut loose: Vec<String> = Vec::new(); // --tb=line entries
    let mut shorts: Vec<ShortEntry> = Vec::new();

    for line in lines {
        if let Some(c) = SECTION.captures(line) {
            let name = &c[1];
            section = match name {
                "FAILURES" => Section::Failures,
                "ERRORS" => Section::Errors,
                n if n.contains("short test summary") => Section::Short,
                n if SUMMARY.is_match(n) => {
                    report.summary = Some(n.to_string());
                    Section::Other
                }
                _ => Section::Other,
            };
            continue;
        }
        if SUMMARY.is_match(line) {
            report.summary = Some(line.trim().to_string()); // -q style
            continue;
        }
        match section {
            Section::Failures | Section::Errors => {
                if let Some(c) = BLOCK.captures(line) {
                    blocks.push(Block {
                        title: c[1].to_string(),
                        lines: Vec::new(),
                        error: section == Section::Errors,
                    });
                } else if let Some(b) = blocks.last_mut() {
                    b.lines.push(line.clone());
                } else if !line.trim().is_empty() {
                    loose.push(line.clone());
                }
            }
            Section::Short => {
                if let Some(c) = SHORT.captures(line) {
                    shorts.push(ShortEntry {
                        id: c[2].to_string(),
                        msg: c.get(3).map(|m| m.as_str().to_string()),
                        error: &c[1] == "ERROR",
                        used: false,
                    });
                }
            }
            _ => {}
        }
    }

    for b in &blocks {
        let f = parse_block(b, &mut shorts);
        report.failures.push(f);
    }
    for line in &loose {
        if let Some(c) = LOC.captures(line) {
            let loc = Loc {
                file: c[1].to_string(),
                line: c[2].parse().unwrap_or(0),
                col: None,
            };
            let msg = c.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();
            let title =
                take_short_by_file(&mut shorts, &loc.file).unwrap_or_else(|| loc.to_string());
            report.failures.push(Failure {
                title,
                location: Some(loc),
                message: vec![msg],
                ..Default::default()
            });
        }
    }
    // --tb=no, or entries whose block we could not parse.
    for s in shorts.iter().filter(|s| !s.used) {
        report.failures.push(Failure {
            title: short_title(s),
            message: s.msg.iter().cloned().collect(),
            ..Default::default()
        });
    }

    report.failed = !report.failures.is_empty()
        || report
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("failed") || s.contains("error"));
    report
}

fn short_title(s: &ShortEntry) -> String {
    if s.error {
        format!("{} (error)", s.id)
    } else {
        s.id.clone()
    }
}

fn take_short_by_file(shorts: &mut [ShortEntry], file: &str) -> Option<String> {
    let s = shorts
        .iter_mut()
        .find(|s| !s.used && file.ends_with(s.id.split("::").next().unwrap_or("")))?;
    s.used = true;
    Some(short_title(s))
}

/// "TestAuth.test_login[a.b]" -> "TestAuth::test_login[a.b]"
fn title_to_id_suffix(title: &str) -> String {
    let mut out = String::with_capacity(title.len() + 4);
    let mut depth = 0;
    for ch in title.chars() {
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if ch == '.' && depth == 0 {
            out.push_str("::");
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_block(b: &Block, shorts: &mut [ShortEntry]) -> Failure {
    let (phase, name) = if let Some(rest) = b.title.strip_prefix("ERROR at setup of ") {
        (Some("error at setup"), rest)
    } else if let Some(rest) = b.title.strip_prefix("ERROR at teardown of ") {
        (Some("error at teardown"), rest)
    } else if let Some(rest) = b.title.strip_prefix("ERROR collecting ") {
        (Some("collection error"), rest)
    } else {
        (if b.error { Some("error") } else { None }, b.title.as_str())
    };
    let suffix = title_to_id_suffix(name);
    let id = shorts
        .iter_mut()
        .find(|s| !s.used && (s.id == name || s.id.ends_with(&format!("::{suffix}"))))
        .map(|s| {
            s.used = true;
            s.id.clone()
        })
        .unwrap_or_else(|| name.to_string());
    let title = match phase {
        Some(p) => format!("{id} ({p})"),
        None => id,
    };

    let mut frames: Vec<Frame> = Vec::new();
    let mut short_style_open = false;
    let mut cur_src: Vec<&str> = Vec::new();
    let mut e_lines: Vec<String> = Vec::new();
    let mut misc: Vec<String> = Vec::new();
    let mut extra: Vec<(String, Vec<String>)> = Vec::new();
    let mut captured: Option<(String, Vec<String>)> = None;

    for line in &b.lines {
        if let Some(c) = CAPTURED.captures(line) {
            extra.extend(captured.take());
            captured = Some((c[1].to_lowercase(), Vec::new()));
            continue;
        }
        if let Some((_, buf)) = captured.as_mut() {
            buf.push(line.clone());
            continue;
        }
        if FRAME_SEP.is_match(line) {
            cur_src.clear();
            short_style_open = false;
            continue;
        }
        if line == "E" || line.starts_with("E ") {
            e_lines.push(line[1..].to_string());
            short_style_open = false;
            continue;
        }
        if let Some(c) = LOC.captures(line) {
            let loc = Loc {
                file: c[1].to_string(),
                line: c[2].parse().unwrap_or(0),
                col: None,
            };
            if let Some(func) = c.get(3) {
                // short style: "path:12: in func" followed by the source line
                frames.push(Frame {
                    loc,
                    func: Some(func.as_str().to_string()),
                    src: None,
                });
                short_style_open = true;
            } else {
                // long style: the location closes the frame's source listing
                let marker = cur_src
                    .iter()
                    .rev()
                    .find(|l| l.starts_with('>'))
                    .map(|l| l[1..].trim().to_string());
                let func = cur_src
                    .iter()
                    .find_map(|l| DEF.captures(l).map(|c| c[1].to_string()));
                frames.push(Frame {
                    loc,
                    func,
                    src: marker,
                });
                short_style_open = false;
            }
            cur_src.clear();
            continue;
        }
        if line.starts_with("    ") || line.starts_with('>') {
            let is_caret = line.trim().chars().all(|c| c == '^' || c == '~');
            if short_style_open && !is_caret {
                if let Some(fr) = frames.last_mut() {
                    if fr.src.is_none() {
                        fr.src = Some(line.trim().to_string());
                    }
                }
            }
            cur_src.push(line);
            continue;
        }
        if line.trim().is_empty()
            || LOCALS.is_match(line)
            || line == "Traceback:"
            || line.starts_with("Hint: ")
        {
            continue;
        }
        misc.push(line.clone());
    }
    extra.extend(captured.take());

    let mut user: Vec<Frame> = frames
        .into_iter()
        .filter(|f| !is_library_path(&f.loc.file))
        .collect();
    if user.len() > 4 {
        user.drain(..user.len() - 4);
    }
    let message = if e_lines.is_empty() {
        cap(tidy(misc), 15)
    } else {
        cap(tidy(dedent(&e_lines)), 30)
    };
    let extra = extra
        .into_iter()
        .map(|(label, lines)| (label, cap(tidy(lines), 12)))
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    Failure {
        title,
        location: user.last().map(|f| f.loc.clone()),
        message,
        frames: user,
        code: None,
        extra,
        dir_hints: Vec::new(),
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
    fn long_style() {
        let r = parse(&fixture("pytest_plain.log"));
        assert_eq!(r.summary.as_deref(), Some("6 failed, 91 passed in 0.11s"));
        assert_eq!(r.failures.len(), 6);
        let f = &r.failures[1];
        assert_eq!(f.title, "tests/test_auth.py::TestAuth::test_expired_token");
        assert_eq!(
            f.message,
            vec!["shop.auth.TokenExpired: token expired at 1000"]
        );
        assert_eq!(
            f.location.as_ref().unwrap().to_string(),
            "src/shop/auth.py:11"
        );
        assert_eq!(f.frames.len(), 2);
        assert_eq!(f.frames[1].func.as_deref(), Some("validate_token"));
        assert_eq!(
            f.frames[1].src.as_deref(),
            Some("raise TokenExpired(f\"token expired at {token['exp']}\")")
        );
        let captured = &r.failures[4].extra;
        assert_eq!(captured[0].0, "captured stdout call");
    }

    #[test]
    fn short_style_matches_long_style() {
        let long = parse(&fixture("pytest_plain.log"));
        let short = parse(&fixture("pytest_short.log"));
        assert_eq!(long.failures.len(), short.failures.len());
        for (a, b) in long.failures.iter().zip(&short.failures) {
            assert_eq!(a.title, b.title);
            assert_eq!(a.location, b.location);
        }
        assert_eq!(
            short.failures[2].frames[1].src.as_deref(),
            Some("rate = rates[code]")
        );
    }

    #[test]
    fn collection_and_fixture_errors() {
        let r = parse(&fixture("pytest_collect_err.log"));
        assert_eq!(r.failures.len(), 1);
        assert_eq!(
            r.failures[0].title,
            "tests/test_broken.py (collection error)"
        );
        assert_eq!(
            r.failures[0].message,
            vec!["ModuleNotFoundError: No module named 'nonexistent_module'"]
        );
        let r = parse(&fixture("pytest_fixture_err.log"));
        assert_eq!(
            r.failures[0].title,
            "tests/test_fixture.py::test_uses_db (error at setup)"
        );
        assert!(r.failed);
    }

    #[test]
    fn tty_output_parses_like_plain() {
        let r = parse(&fixture("pytest_tty.log"));
        assert_eq!(r.failures.len(), 6);
        assert_eq!(r.summary.as_deref(), Some("6 failed, 91 passed in 0.11s"));
    }
}
