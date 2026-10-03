//! sanitize -> detect -> parse -> attach code -> render -> footer.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{self, ContextOpts, Resolver};
use crate::detect::detect;
use crate::engines::{self, Ctx};
use crate::report::{render, Engine, RenderOpts};
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

    let mut text = render(
        &report,
        &RenderOpts {
            max_failures: opts.max_failures,
            exit_code: input.exit_code,
        },
    );
    // Absolute paths inside the project are noise: make them relative.
    if !report.passthrough {
        let prefix = format!("{}{}", opts.root.display(), std::path::MAIN_SEPARATOR);
        if prefix.len() > 2 {
            text = text.replace(&prefix, "");
        }
    }

    let before = estimate(&full);
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
