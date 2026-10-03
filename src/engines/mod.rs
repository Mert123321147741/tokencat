//! Parser adapters. Each one turns sanitized lines into a [`Report`].

mod cargo;
pub mod generic;
mod go;
mod js;
mod pytest;

use crate::report::{Engine, Report};

pub struct Ctx {
    pub exit_code: Option<i32>,
    /// Lines of context kept around interesting lines by the generic engine.
    pub context_lines: usize,
    /// Outputs with at most this many lines pass through untouched.
    pub small: usize,
    /// Cap for the generic engine's kept lines.
    pub max_lines: usize,
}

pub fn parse(engine: Engine, lines: &[String], ctx: &Ctx) -> Report {
    let mut report = match engine {
        Engine::Pytest => pytest::parse(lines),
        Engine::Jest => js::parse_jest(lines),
        Engine::Vitest => js::parse_vitest(lines),
        Engine::Go => go::parse(lines),
        Engine::Cargo => cargo::parse(lines),
        Engine::Generic => return generic::parse(lines, ctx),
    };
    if ctx.exit_code.is_some_and(|c| c != 0) {
        report.failed = true;
    }
    // Safety net: the runner failed but the adapter recognised nothing (a
    // usage error, a crash before tests ran, an unfamiliar format). Never hide
    // that; fall back to generic extraction of the error lines.
    if report.failures.is_empty() && report.failed {
        let fallback = generic::parse(lines, ctx);
        report.body = fallback.body;
        report.hint_locs = fallback.hint_locs;
        report.notes.extend(fallback.notes);
    }
    report
}
