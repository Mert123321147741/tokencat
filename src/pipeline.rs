//! sanitize -> detect -> parse -> attach code -> render -> footer.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{self, ContextOpts, Resolver};
use crate::detect::detect;
use crate::engines::{self, Ctx};
use crate::report::{render, Engine, RenderOpts, Report};
use crate::sanitize::sanitize;
use crate::tokens::{estimate, Footer};

pub struct Options {
    pub engine: Option<Engine>,
    pub context_lines: usize,
    pub snippet_lines: usize,
    pub max_snippets: usize,
    pub max_failures: usize,
    pub max_lines: usize,
    pub small: usize,
    pub stats: bool,
    pub price_per_mtok: f64,
    /// Where to keep the full log when lines were dropped; None disables it.
    pub log_dir: Option<PathBuf>,
    pub raw: bool,
    pub root: PathBuf,
}

pub struct Input<'a> {
    pub bytes: &'a [u8],
    pub exit_code: Option<i32>,
    pub cmd: Option<&'a [String]>,
}

pub fn process(input: &Input, opts: &Options) -> String {
    let lines = sanitize(input.bytes);
    let full = lines.join("\n");
    if opts.raw {
        return if full.is_empty() { full } else { full + "\n" };
    }

    let engine = opts.engine.unwrap_or_else(|| detect(&lines, input.cmd));
    let ctx = Ctx {
        exit_code: input.exit_code,
        context_lines: opts.context_lines,
        small: opts.small,
        max_lines: opts.max_lines,
    };
    let mut report = engines::parse(engine, &lines, &ctx);

    let resolver = Resolver::new(opts.root.clone());
    let copts = ContextOpts {
        snippet_lines: opts.snippet_lines,
        max_snippets: opts.max_snippets,
    };
    context::attach(&mut report, &resolver, &copts);
    if report.failures.is_empty() && !report.hint_locs.is_empty() {
        let locs = std::mem::take(&mut report.hint_locs);
        context::attach_generic(&mut report, &locs, &resolver, &copts);
    }

    let finish = |report: &Report| {
        let mut text = render(
            report,
            &RenderOpts {
                max_failures: opts.max_failures,
                exit_code: input.exit_code,
            },
        );
        // Absolute paths inside the project are noise: make them relative.
        if !report.passthrough || !report.snippets.is_empty() {
            let prefix = format!("{}{}", opts.root.display(), std::path::MAIN_SEPARATOR);
            if prefix.len() > 2 {
                text = text.replace(&prefix, "");
            }
        }
        text
    };

    let before = estimate(&full);
    let mut text = finish(&report);
    // The report must never cost more than the output it replaces. Source
    // read from disk is deliberate extra context, so judge the report
    // without it; if headings and structure alone are bigger (an already
    // terse output), show the cleaned output itself, plus that source.
    if !report.passthrough && estimate(&finish(&without_disk_code(&report))) > before {
        report = into_passthrough(report, &lines);
        text = finish(&report);
    }
    let mut text = shorten_long_lines(&text);
    let after = estimate(&text);
    // The footer costs ~30 tokens: only print it when it pays for itself,
    // or when a failure was pruned and the full log path may be needed.
    let saved = before.saturating_sub(after);
    let worth_it = saved >= 100 || (saved > 0 && report.failed && !report.passthrough);
    if opts.stats && worth_it {
        let dropped_lines = !report.passthrough;
        let log = if dropped_lines {
            opts.log_dir.as_ref().and_then(|d| save_log(d, &full))
        } else {
            None
        };
        let footer = Footer {
            before,
            after,
            price_per_mtok: opts.price_per_mtok,
            log_path: log.as_deref(),
        };
        if text.lines().count() > 1 && !text.ends_with("\n\n") {
            text.push('\n');
        }
        text.push_str(&footer.render());
        text.push('\n');
    }
    text
}

fn without_disk_code(report: &Report) -> Report {
    let mut bare = report.clone();
    for f in &mut bare.failures {
        if f.code.as_ref().is_some_and(|c| c.from_disk) {
            f.code = None;
        }
    }
    bare.snippets.clear();
    bare
}

fn into_passthrough(mut report: Report, lines: &[String]) -> Report {
    let failures = std::mem::take(&mut report.failures);
    for f in failures {
        if let (Some(loc), Some(code)) = (f.location, f.code) {
            if code.from_disk && !report.snippets.iter().any(|(l, _)| *l == loc) {
                report.snippets.push((loc, code));
            }
        }
    }
    report.body = lines.to_vec();
    report.passthrough = true;
    report
}

/// Lines longer than this (minified bundles, JSON dumps, base64 blobs) keep
/// their head and tail only.
const MAX_LINE_CHARS: usize = 1000;
const KEEP_HEAD: usize = 700;
const KEEP_TAIL: usize = 200;

fn shorten_long_lines(text: &str) -> String {
    if text.lines().all(|l| l.len() <= MAX_LINE_CHARS) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len().min(1 << 20));
    for line in text.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let n = body.chars().count();
        if n <= MAX_LINE_CHARS {
            out.push_str(line);
            continue;
        }
        let head_end = body
            .char_indices()
            .nth(KEEP_HEAD)
            .map_or(body.len(), |(i, _)| i);
        let tail_start = body
            .char_indices()
            .nth(n - KEEP_TAIL)
            .map_or(body.len(), |(i, _)| i);
        out.push_str(&body[..head_end]);
        out.push_str(&format!(" … [{} chars] … ", n - KEEP_HEAD - KEEP_TAIL));
        out.push_str(&body[tail_start..]);
        out.push_str(nl);
    }
    out
}

fn save_log(dir: &PathBuf, full: &str) -> Option<String> {
    fs::create_dir_all(dir).ok()?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("{secs}-{}.log", std::process::id()));
    fs::write(&path, format!("{full}\n")).ok()?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options {
            engine: None,
            context_lines: 3,
            snippet_lines: 5,
            max_snippets: 8,
            max_failures: 10,
            max_lines: 150,
            small: 30,
            stats: true,
            price_per_mtok: 4.0,
            log_dir: None,
            raw: false,
            root: PathBuf::from("/nonexistent"),
        }
    }

    #[test]
    fn long_lines_keep_head_and_tail() {
        let line = format!("error: {}END", "x".repeat(5000));
        let out = shorten_long_lines(&format!("short\n{line}\n"));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "short");
        assert!(lines[1].starts_with("error: xxx"));
        assert!(lines[1].ends_with("xxxEND"));
        assert!(lines[1].contains(" … [4110 chars] … "));
        assert_eq!(
            lines[1].chars().count(),
            900 + " … [4110 chars] … ".chars().count()
        );
        // Multi-byte characters are cut on character boundaries.
        let wide = "é".repeat(3000);
        assert!(shorten_long_lines(&wide).contains("[2100 chars]"));
    }

    #[test]
    fn report_never_outgrows_terse_output() {
        // Two lines of `go test` output: a structured report would be longer.
        let bytes = b"# example.com/x\nx/x.go:3:9: undefined: y\nFAIL\texample.com/x [build failed]\nFAIL\n";
        let input = Input {
            bytes,
            exit_code: Some(1),
            cmd: None,
        };
        let out = process(&input, &opts());
        assert_eq!(out, String::from_utf8_lossy(bytes));
    }

    #[test]
    fn huge_line_in_failing_output_is_cut() {
        let mut log = String::new();
        for i in 0..50 {
            log.push_str(&format!("step {i} done\n"));
        }
        log.push_str(&format!("Error: bundle failed {}\n", "A".repeat(200_000)));
        let input = Input {
            bytes: log.as_bytes(),
            exit_code: Some(1),
            cmd: None,
        };
        let out = process(&input, &opts());
        assert!(out.len() < 3000, "{} bytes", out.len());
        assert!(out.contains("Error: bundle failed AAA"));
    }
}
