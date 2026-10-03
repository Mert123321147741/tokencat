//! `tokencat hook claude`: a Claude Code `PreToolUse` hook that rewrites
//! test and build commands so they run through `tokencat run`, without the
//! agent having to remember to.
//!
//! The hook only rewrites commands it recognises and understands; anything
//! else (and any input it cannot parse) passes through untouched. It never
//! sets a permission decision, so Claude Code's normal permission rules
//! apply to the rewritten command.

use std::io::{self, Read};

use serde_json::{json, Value};

pub fn claude() {
    let mut raw = String::new();
    if io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(out) = claude_output(&raw) {
        println!("{out}");
    }
}

/// The JSON to print for a hook input, or None to leave the call alone.
fn claude_output(raw: &str) -> Option<String> {
    let input: Value = serde_json::from_str(raw).ok()?;
    if input.get("tool_name")?.as_str()? != "Bash" {
        return None;
    }
    let tool_input = input.get("tool_input")?.as_object()?;
    if tool_input
        .get("run_in_background")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let command = tool_input.get("command")?.as_str()?;
    let rewritten = rewrite(command)?;
    // updatedInput replaces the whole input object, so carry every field.
    let mut updated = tool_input.clone();
    updated.insert("command".into(), Value::String(rewritten));
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "updatedInput": updated,
        }
    });
    Some(out.to_string())
}

/// `pytest -x 2>&1 | tail -40` -> `tokencat run -- pytest -x`.
pub fn rewrite(command: &str) -> Option<String> {
    let mut cmd = command.trim();
    if cmd.is_empty() || cmd.contains("tokencat") || cmd.contains('\n') {
        return None;
    }
    // A trailing `| tail -n 50` / `| head -30` only exists to keep the
    // output short, which is tokencat's job, and it hides the exit code.
    if let Some((left, right)) = cmd.rsplit_once('|') {
        if !left.ends_with('|') && is_trimmer(right.trim()) {
            cmd = left.trim_end();
        }
    }
    // tokencat captures stdout and stderr together anyway.
    if let Some(left) = cmd.strip_suffix("2>&1") {
        cmd = left.trim_end();
    }
    // Pipes, redirections, background jobs, command lists and
    // substitutions change what the agent sees; leave those alone.
    let unchained = cmd.replace("&&", " ");
    if unchained.contains(['|', ';', '<', '>', '&', '`']) || unchained.contains("$(") {
        return None;
    }
    // `cd web && npm test` is fine; anything else chained in front would be
    // hidden inside a quoted string from permission checks, so skip it.
    let mut segments: Vec<&str> = cmd.split("&&").map(str::trim).collect();
    let last = segments.pop()?;
    if !segments.iter().all(|s| is_cd(s)) || !is_test_command(last) {
        return None;
    }
    let env_prefix = first_word(last).contains('=');
    let special = cmd.contains([
        '\'', '"', '\\', '$', '*', '?', '[', ']', '{', '}', '~', '#', '(', ')', '%', '^', '!',
    ]);
    // On Windows the agent's shell is Git Bash but `tokencat run` hands a
    // command string to cmd.exe, which quotes and expands differently. Only
    // rewrite commands both read the same way.
    if cfg!(windows) && (env_prefix || special) {
        return None;
    }
    let simple = segments.is_empty() && !env_prefix && !special;
    Some(if simple {
        format!("tokencat run -- {cmd}")
    } else {
        format!("tokencat run -- '{}'", cmd.replace('\'', r"'\''"))
    })
}

fn is_cd(segment: &str) -> bool {
    let words: Vec<&str> = segment.split_whitespace().collect();
    matches!(words.as_slice(), ["cd", _])
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn is_trimmer(stage: &str) -> bool {
    let mut words = stage.split_whitespace();
    if !matches!(words.next(), Some("tail" | "head")) {
        return false;
    }
    let rest: Vec<&str> = words.collect();
    match rest.as_slice() {
        [] => true,
        [n] => n.strip_prefix('-').is_some_and(all_digits),
        ["-n", n] => all_digits(n),
        _ => false,
    }
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Recognises the test runners and build tools tokencat has parsers for,
/// behind the usual launchers (`uv run`, `npx`, `python -m`, env vars).
fn is_test_command(segment: &str) -> bool {
    let mut words: Vec<&str> = segment
        .split_whitespace()
        .skip_while(|w| w.contains('=') && !w.starts_with('-'))
        .collect();
    loop {
        match words.as_slice() {
            ["uv" | "poetry" | "pipenv" | "pdm" | "hatch" | "rye", "run", ..] => {
                words.drain(..2);
            }
            ["pnpm" | "yarn", "exec" | "dlx", ..] => {
                words.drain(..2);
            }
            ["npx" | "bunx" | "time", ..] => {
                words.drain(..1);
            }
            _ => break,
        }
    }
    let prog = words.first().map(|w| w.rsplit('/').next().unwrap_or(w));
    let arg = |i: usize| words.get(i).copied().unwrap_or("");
    match prog {
        Some("pytest" | "py.test" | "jest" | "vitest" | "tsc" | "mypy" | "nose2") => true,
        Some(p) if p.starts_with("python") => match (arg(1), arg(2)) {
            ("-m", "pytest" | "unittest" | "mypy" | "nose2") => true,
            (script, "test") if script.ends_with("manage.py") => true,
            (script, _) if script.ends_with("runtests.py") => true,
            _ => false,
        },
        Some("manage.py" | "django-admin") => arg(1) == "test",
        Some("npm" | "pnpm" | "yarn" | "bun") => {
            matches!(arg(1), "test" | "t")
                || (arg(1) == "run" && (arg(2) == "test" || arg(2).starts_with("test:")))
        }
        Some("go") => matches!(arg(1), "test" | "build" | "vet"),
        Some("cargo") => matches!(arg(1), "test" | "build" | "check" | "clippy" | "nextest"),
        Some("make") => matches!(arg(1), "test" | "check"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_test_commands() {
        let cases = [
            ("pytest -x tests/", "tokencat run -- pytest -x tests/"),
            ("python -m pytest -q", "tokencat run -- python -m pytest -q"),
            ("uv run pytest", "tokencat run -- uv run pytest"),
            ("npm test", "tokencat run -- npm test"),
            ("npm run test:unit", "tokencat run -- npm run test:unit"),
            ("npx vitest run", "tokencat run -- npx vitest run"),
            ("go test ./...", "tokencat run -- go test ./..."),
            (
                "cargo test --workspace",
                "tokencat run -- cargo test --workspace",
            ),
            (
                "python manage.py test shop",
                "tokencat run -- python manage.py test shop",
            ),
            (
                "python3 -m unittest -v",
                "tokencat run -- python3 -m unittest -v",
            ),
            ("pytest 2>&1", "tokencat run -- pytest"),
            ("pytest -x 2>&1 | tail -40", "tokencat run -- pytest -x"),
            (
                "go test ./... | head -n 100",
                "tokencat run -- go test ./...",
            ),
            ("cd web && npm test", "tokencat run -- 'cd web && npm test'"),
        ];
        for (input, want) in cases {
            assert_eq!(rewrite(input).as_deref(), Some(want), "{input}");
        }
    }

    #[test]
    fn shell_syntax_is_quoted_on_unix_and_skipped_on_windows() {
        let cases = [
            ("CI=1 npx jest", "tokencat run -- 'CI=1 npx jest'"),
            (
                "pytest -k 'cart and not slow'",
                r"tokencat run -- 'pytest -k '\''cart and not slow'\'''",
            ),
        ];
        for (input, want) in cases {
            let want = if cfg!(windows) { None } else { Some(want) };
            assert_eq!(rewrite(input).as_deref(), want, "{input}");
        }
    }

    #[test]
    fn leaves_other_commands_alone() {
        for input in [
            "ls -la",
            "git status",
            "npm install",
            "cat pytest.ini",
            "pytest | grep FAILED",
            "pytest > out.txt",
            "pytest; echo done",
            "pytest &",
            "npm test && git push",
            "rm -rf build && pytest",
            "cd a b && pytest",
            "tokencat run -- pytest",
            "echo $(go test ./...)",
            "pytest\nrm -rf build",
            "",
        ] {
            assert_eq!(rewrite(input), None, "{input:?}");
        }
    }

    #[test]
    fn hook_output_keeps_other_fields() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"pytest -q","description":"Run tests","timeout":120000}}"#;
        let out: Value = serde_json::from_str(&claude_output(input).unwrap()).unwrap();
        let updated = &out["hookSpecificOutput"]["updatedInput"];
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert!(out["hookSpecificOutput"]
            .get("permissionDecision")
            .is_none());
        assert_eq!(updated["command"], "tokencat run -- pytest -q");
        assert_eq!(updated["description"], "Run tests");
        assert_eq!(updated["timeout"], 120000);
    }

    #[test]
    fn hook_ignores_what_it_should() {
        for input in [
            r#"{"tool_name":"Read","tool_input":{"file_path":"a.py"}}"#,
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            r#"{"tool_name":"Bash","tool_input":{"command":"pytest","run_in_background":true}}"#,
            "not json",
        ] {
            assert_eq!(claude_output(input), None, "{input}");
        }
    }
}
