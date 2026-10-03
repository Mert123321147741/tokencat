//! End-to-end tests against the built binary: exit code fidelity, signals,
//! capture edge cases, and golden outputs for every fixture log.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tokencat"));
    c.env_remove("TOKENCAT_DISABLE")
        .env_remove("TOKENCAT_ENGINE")
        .env_remove("TOKENCAT_LOG_DIR")
        .env_remove("TOKENCAT_PRICE");
    c
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[cfg(unix)]
fn run(args: &[&str]) -> Output {
    bin()
        .arg("--no-log")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn pipe(input: &[u8], args: &[&str], cwd: &Path) -> Output {
    let mut child = bin()
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn exit_codes_pass_through_exactly() {
        for code in [0, 1, 2, 3, 42, 101, 255] {
            for pty in [true, false] {
                let script = format!("echo out; exit {code}");
                let mut args = vec!["run"];
                if !pty {
                    args.push("--no-pty");
                }
                args.extend(["--", "sh", "-c", &script]);
                let o = run(&args);
                assert_eq!(o.status.code(), Some(code), "code {code}, pty {pty}");
            }
        }
    }

    #[test]
    fn killed_by_signal_maps_to_128_plus_signal() {
        let o = run(&["run", "--", "sh", "-c", "echo hi; kill -TERM $$"]);
        assert_eq!(o.status.code(), Some(143));
        let o = run(&["run", "--no-pty", "--", "sh", "-c", "kill -KILL $$"]);
        assert_eq!(o.status.code(), Some(137));
    }

    #[test]
    fn missing_command_is_127() {
        let o = run(&["run", "--", "definitely-not-a-command-xyz"]);
        assert_eq!(o.status.code(), Some(127));
        assert!(String::from_utf8_lossy(&o.stderr).contains("command not found"));
    }

    #[test]
    fn single_string_runs_through_the_shell() {
        let o = run(&["run", "--", "echo hello && exit 4"]);
        assert_eq!(o.status.code(), Some(4));
        assert!(stdout(&o).contains("hello"));
    }

    #[test]
    fn pty_keeps_stdout_stderr_order_and_reports_a_tty() {
        let o = run(&[
            "run",
            "--",
            "sh",
            "-c",
            "echo one; echo two >&2; echo three; [ -t 1 ] && echo is-tty",
        ]);
        assert_eq!(stdout(&o), "one\ntwo\nthree\nis-tty\n");
        let o = run(&[
            "run",
            "--no-pty",
            "--",
            "sh",
            "-c",
            "[ -t 1 ] || echo not-tty",
        ]);
        assert_eq!(stdout(&o), "not-tty\n");
    }

    #[test]
    fn background_grandchild_does_not_block_exit() {
        let start = Instant::now();
        let o = run(&["run", "--", "sh", "-c", "sleep 30 & echo started"]);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(o.status.code(), Some(0));
        assert!(stdout(&o).contains("started"));
    }

    #[test]
    fn large_output_is_captured_and_pruned() {
        let o = run(&[
            "run",
            "--",
            "sh",
            "-c",
            "i=0; while [ $i -lt 20000 ]; do echo \"line $i ok\"; i=$((i+1)); done; echo 'src/app.c:10:3: error: boom'; exit 2",
        ]);
        assert_eq!(o.status.code(), Some(2));
        let out = stdout(&o);
        assert!(out.contains("src/app.c:10:3: error: boom"), "{out}");
        assert!(out.contains("lines omitted"));
        assert!(out.lines().count() < 40, "{out}");
    }

    #[test]
    fn disable_env_passes_output_through_without_a_pty() {
        let o = bin()
            .env("TOKENCAT_DISABLE", "1")
            .args([
                "run",
                "--",
                "sh",
                "-c",
                "printf '\\033[31mred\\033[0m\\n'; exit 5",
            ])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(5));
        assert_eq!(o.stdout, b"\x1b[31mred\x1b[0m\n");
    }

    #[test]
    fn children_get_no_color_hint() {
        let o = run(&[
            "run",
            "--",
            "sh",
            "-c",
            "echo \"$NO_COLOR/$CARGO_TERM_COLOR\"",
        ]);
        assert_eq!(stdout(&o), "1/never\n");
    }
}

#[test]
fn pipe_mode_exits_zero_and_strips_ansi() {
    let o = pipe(b"\x1b[32mok\x1b[0m\n", &["--no-log"], &manifest());
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "ok\n");
}

#[test]
fn raw_mode_keeps_every_line() {
    let input: String = (0..100)
        .map(|i| format!("\x1b[1mline {i}\x1b[0m\n"))
        .collect();
    let o = pipe(input.as_bytes(), &["--raw"], &manifest());
    assert_eq!(stdout(&o).lines().count(), 100);
    assert!(!stdout(&o).contains('\x1b'));
}

#[test]
fn saves_full_log_when_pruning() {
    let dir = std::env::temp_dir().join(format!("tokencat-logs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let log = fs::read(manifest().join("tests/fixtures/pytest_plain.log")).unwrap();
    let o = pipe(&log, &["--log-dir", dir.to_str().unwrap()], &manifest());
    let out = stdout(&o);
    let path = out
        .split("full log: ")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("footer with log path");
    let saved = fs::read_to_string(path).unwrap();
    assert!(saved.contains("test session starts"));
    assert!(saved.contains("tests/test_cart_bulk.py"));
    let _ = fs::remove_dir_all(&dir);
}

/// Every fixture log is rendered and compared with tests/golden/<name>.md.
/// Run with UPDATE_GOLDEN=1 to rewrite the expected files after a change.
#[test]
fn golden_outputs() {
    let root = manifest();
    let fixtures = root.join("tests/fixtures");
    let golden = root.join("tests/golden");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    if update {
        fs::create_dir_all(&golden).unwrap();
    }
    let mut names: Vec<String> = fs::read_dir(&fixtures)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".log"))
        .collect();
    names.sort();
    let mut failures = Vec::new();
    for name in names {
        let project = match name.split('_').next().unwrap() {
            "pytest" => "py",
            "jest" => "js",
            "vitest" => "vt",
            "go" if name.starts_with("go_build") => "gobuild",
            "go" => "go",
            "cargo" if name.contains("compile_err") => "rs_err",
            "cargo" => "rs",
            "unittest" => "ut",
            _ => "",
        };
        let cwd = fixtures.join("projects").join(project);
        let input = fs::read(fixtures.join(&name)).unwrap();
        let o = pipe(&input, &["--no-log"], &cwd);
        let got = stdout(&o);
        let path = golden.join(name.replace(".log", ".md"));
        if update {
            fs::write(&path, &got).unwrap();
            continue;
        }
        let want = fs::read_to_string(&path).unwrap_or_default();
        if got != want {
            failures.push(format!("--- {name}\n{got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatch (UPDATE_GOLDEN=1 to accept):\n{}",
        failures.join("\n")
    );
}

#[test]
fn claude_hook_rewrites_only_test_commands() {
    let hook = |json: &str| pipe(json.as_bytes(), &["hook", "claude"], &manifest());
    let o = hook(r#"{"tool_name":"Bash","tool_input":{"command":"pytest -q","timeout":60000}}"#);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(
        out.contains(r#""command":"tokencat run -- pytest -q""#),
        "{out}"
    );
    assert!(out.contains(r#""timeout":60000"#), "{out}");

    for json in [
        r#"{"tool_name":"Bash","tool_input":{"command":"ls -la"}}"#,
        r#"{"tool_name":"Edit","tool_input":{}}"#,
        "garbage",
    ] {
        let o = hook(json);
        assert!(o.status.success(), "{json}");
        assert!(o.stdout.is_empty(), "{json}: {}", stdout(&o));
    }
}
