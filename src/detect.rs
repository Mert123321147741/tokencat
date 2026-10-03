//! Picks the parser adapter from the output itself (and the command, when
//! we ran it). Detection uses cheap substring checks, no regexes, so it adds
//! next to nothing to startup time.

use crate::report::Engine;

const MIN_SCORE: u32 = 5;

pub fn detect(lines: &[String], cmd: Option<&[String]>) -> Engine {
    let mut score = [0u32; 5]; // pytest, jest, vitest, go, cargo
    const PY: usize = 0;
    const JEST: usize = 1;
    const VITEST: usize = 2;
    const GO: usize = 3;
    const CARGO: usize = 4;

    // Look at the head and the tail; summaries live at the end.
    let head = lines.iter().take(3000);
    let tail = lines.iter().skip(lines.len().saturating_sub(300).max(3000));
    for line in head.chain(tail) {
        let t = line.trim_start();
        // pytest
        if line.starts_with('=') {
            if line.contains(" test session starts ") || line.contains(" short test summary info ")
            {
                score[PY] += 10;
            } else if line.contains(" FAILURES ")
                || line.contains(" ERRORS ")
                || ((line.contains(" passed")
                    || line.contains(" failed")
                    || line.contains(" error"))
                    && line.contains(" in "))
            {
                score[PY] += 3;
            }
        } else if line.starts_with("platform ") && line.contains(" -- Python ") {
            score[PY] += 10;
        } else if (line.starts_with("FAILED ") || line.starts_with("ERROR ")) && line.contains("::")
        {
            score[PY] += 3;
        }
        // jest
        if t.starts_with("Test Suites: ") || t.starts_with("Tests:       ") {
            score[JEST] += 6;
        } else if (t.starts_with("FAIL ") || t.starts_with("PASS ")) && !t.contains(" > ") {
            score[JEST] += 2;
        } else if t.starts_with("● ") && t.contains(" › ") {
            score[JEST] += 3;
        }
        // vitest
        if t.starts_with("RUN  v")
            || t.starts_with("DEV  v")
            || t.starts_with("Test Files  ")
            || (t.starts_with('⎯') && (t.contains("Failed Tests") || t.contains("Failed Suites")))
        {
            score[VITEST] += 8;
        } else if t.starts_with("FAIL ") && t.contains(" > ") {
            score[VITEST] += 3;
        }
        // go
        if t.starts_with("--- FAIL: ") || t.starts_with("--- PASS: ") {
            score[GO] += 4;
        } else if line.starts_with("=== RUN ") {
            score[GO] += 2;
        } else if (line.starts_with("ok  \t")
            || line.starts_with("FAIL\t")
            || line.starts_with("?   \t"))
            && line.contains('.')
        {
            score[GO] += 5;
        } else if (line.starts_with("# ") && is_go_build_error_follow(line))
            || line.split(".go:").nth(1).is_some_and(starts_with_digit)
        {
            score[GO] += 1;
        }
        // cargo / rustc
        if line.starts_with("error[E") || line.starts_with("warning: unused") {
            score[CARGO] += 5;
        } else if line.starts_with("test result: ") {
            score[CARGO] += 6;
        } else if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            score[CARGO] += 5;
        } else if line.starts_with("running ")
            && (line.ends_with(" tests") || line.ends_with(" test"))
        {
            score[CARGO] += 2;
        } else if t.starts_with("Compiling ") && t.contains(" v") {
            score[CARGO] += 1;
        } else if t.starts_with("--> ") && line.contains(".rs:") {
            score[CARGO] += 2;
        }
    }

    // The command line is a strong hint, but output wins when it is clear.
    if let Some(cmd) = cmd {
        let joined = cmd.join(" ").to_lowercase();
        let words: Vec<&str> = joined
            .split(|c: char| c.is_whitespace() || c == '/')
            .collect();
        let has = |w: &str| words.contains(&w);
        if has("pytest") || has("py.test") || joined.contains("-m pytest") {
            score[PY] += 6;
        }
        if has("jest") {
            score[JEST] += 6;
        }
        if has("vitest") {
            score[VITEST] += 6;
        }
        if has("go") && (has("test") || has("build") || has("vet") || has("run")) {
            score[GO] += 6;
        }
        if has("cargo") || has("rustc") {
            score[CARGO] += 6;
        }
    }

    // Vitest prints some jest-like lines; prefer it when its markers exist.
    if score[VITEST] >= 8 && score[JEST] < score[VITEST] + 6 {
        score[JEST] = 0;
    }

    let (best, &max) = score.iter().enumerate().max_by_key(|(_, s)| **s).unwrap();
    if max < MIN_SCORE {
        return Engine::Generic;
    }
    match best {
        PY => Engine::Pytest,
        JEST => Engine::Jest,
        VITEST => Engine::Vitest,
        GO => Engine::Go,
        _ => Engine::Cargo,
    }
}

fn starts_with_digit(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn is_go_build_error_follow(line: &str) -> bool {
    // "# example.com/pkg" or "# example.com/pkg [example.com/pkg.test]"
    let rest = &line[2..];
    let first = rest.split(' ').next().unwrap_or("");
    first.contains('/') || first.contains('.')
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
    fn detects_fixtures() {
        let cases = [
            ("pytest_plain.log", Engine::Pytest),
            ("pytest_tty.log", Engine::Pytest),
            ("pytest_short.log", Engine::Pytest),
            ("pytest_collect_err.log", Engine::Pytest),
            ("jest_plain.log", Engine::Jest),
            ("jest_tty.log", Engine::Jest),
            ("vitest_plain.log", Engine::Vitest),
            ("vitest_tty.log", Engine::Vitest),
            ("go_plain.log", Engine::Go),
            ("go_verbose.log", Engine::Go),
            ("go_build.log", Engine::Go),
            ("cargo_test.log", Engine::Cargo),
            ("cargo_compile_err.log", Engine::Cargo),
        ];
        for (name, want) in cases {
            assert_eq!(detect(&fixture(name), None), want, "{name}");
        }
    }

    #[test]
    fn unknown_output_is_generic() {
        let lines: Vec<String> = ["make: *** [all] Error 2", "hello"]
            .map(String::from)
            .to_vec();
        assert_eq!(detect(&lines, None), Engine::Generic);
    }

    #[test]
    fn command_hint_breaks_ties() {
        let lines: Vec<String> = vec!["something odd".into()];
        let cmd = vec![
            "python".to_string(),
            "-m".into(),
            "pytest".into(),
            "-x".into(),
        ];
        assert_eq!(detect(&lines, Some(&cmd)), Engine::Pytest);
    }
}
