mod capture;
mod context;
mod detect;
mod engines;
mod pipeline;
mod report;
mod sanitize;
mod tokens;

use std::io::{self, IsTerminal, Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::report::Engine;

const ABOUT: &str = "Prune test runner and compiler output down to what an AI coding agent needs.";

const LONG_ABOUT: &str = "\
Prune test runner and compiler output down to what an AI coding agent needs.

tokencat strips ANSI codes and progress bars, detects the tool (pytest, jest,
vitest, go, cargo, or a generic fallback), keeps only failures with their
messages, diffs, stack frames and a few lines of source, and prints a short
Markdown report with a token savings line.

  cmd | tokencat              filter output from a pipe (exit status is 0)
  tokencat run -- cmd ...     run cmd, filter its output, exit with its code

Set TOKENCAT_DISABLE=1 to pass output through untouched.";

#[derive(Parser)]
#[command(name = "tokencat", version, about = ABOUT, long_about = LONG_ABOUT)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    #[command(flatten)]
    opts: Opts,

    /// Read from FILE instead of stdin.
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a command, prune its output and exit with the command's exit code.
    Run {
        /// Capture through plain pipes instead of a pseudo-terminal.
        #[arg(long)]
        no_pty: bool,

        /// The command to run. A single argument containing spaces or shell
        /// syntax is run through the shell.
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "CMD"
        )]
        cmd: Vec<String>,
    },
}

#[derive(Args)]
struct Opts {
    /// Parser to use instead of auto-detection.
    #[arg(short, long, global = true, value_enum, env = "TOKENCAT_ENGINE")]
    engine: Option<Engine>,

    /// Lines kept around each error line by the generic parser.
    #[arg(
        short = 'C',
        long,
        global = true,
        default_value_t = 3,
        value_name = "N"
    )]
    context: usize,

    /// Lines of source code attached to a failure (0 disables).
    #[arg(
        long,
        global = true,
        default_value_t = 5,
        value_name = "N",
        env = "TOKENCAT_SNIPPET_LINES"
    )]
    snippet_lines: usize,

    /// Failures shown in full; the rest are listed by name.
    #[arg(
        long,
        global = true,
        default_value_t = 10,
        value_name = "N",
        env = "TOKENCAT_MAX_FAILURES"
    )]
    max_failures: usize,

    /// Cap on lines kept by the generic parser.
    #[arg(long, global = true, default_value_t = 150, value_name = "N")]
    max_lines: usize,

    /// Outputs with at most N lines are passed through unchanged.
    #[arg(long, global = true, default_value_t = 30, value_name = "N")]
    small: usize,

    /// Do not print the token savings line.
    #[arg(long, global = true)]
    no_stats: bool,

    /// Input price in USD per million tokens, for the savings estimate.
    #[arg(
        long,
        global = true,
        default_value_t = 4.0,
        value_name = "USD",
        env = "TOKENCAT_PRICE"
    )]
    price: f64,

    /// Directory for full logs when output was pruned.
    #[arg(long, global = true, value_name = "DIR", env = "TOKENCAT_LOG_DIR")]
    log_dir: Option<PathBuf>,

    /// Never write the full log to disk.
    #[arg(long, global = true)]
    no_log: bool,

    /// Only strip escape codes and progress redraws; keep every line.
    #[arg(long, global = true)]
    raw: bool,
}

impl Opts {
    fn pipeline(&self) -> pipeline::Options {
        let log_dir = if self.no_log {
            None
        } else {
            Some(
                self.log_dir
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("tokencat")),
            )
        };
        pipeline::Options {
            engine: self.engine,
            context_lines: self.context,
            snippet_lines: self.snippet_lines,
            max_snippets: 8,
            max_failures: self.max_failures.max(1),
            max_lines: self.max_lines,
            small: self.small,
            stats: !self.no_stats,
            price_per_mtok: self.price,
            log_dir,
            raw: self.raw,
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

fn disabled() -> bool {
    std::env::var("TOKENCAT_DISABLE").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn write_out(bytes: &[u8]) {
    let mut out = io::stdout().lock();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

/// Runs the pipeline, but never lets a parser bug swallow the output: on a
/// panic we print the sanitized log instead.
fn process_safely(input: &pipeline::Input, opts: &pipeline::Options) -> String {
    panic::set_hook(Box::new(|_| {}));
    match panic::catch_unwind(AssertUnwindSafe(|| pipeline::process(input, opts))) {
        Ok(text) => text,
        Err(_) => {
            let lines = sanitize::sanitize(input.bytes);
            let mut text = lines.join("\n");
            text.push_str("\n[tokencat: internal error while parsing; showing full output]\n");
            text
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Run { no_pty, cmd }) => {
            let captured = match capture::run(&cmd, !no_pty && !disabled()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e.message);
                    return ExitCode::from(e.code as u8);
                }
            };
            if disabled() {
                write_out(&captured.output);
            } else {
                let input = pipeline::Input {
                    bytes: &captured.output,
                    exit_code: Some(captured.code),
                    cmd: Some(&cmd),
                };
                let text = process_safely(&input, &cli.opts.pipeline());
                write_out(text.as_bytes());
            }
            ExitCode::from((captured.code & 0xff) as u8)
        }
        None => {
            let mut bytes = Vec::new();
            let read = match &cli.file {
                Some(path) => std::fs::File::open(path).and_then(|mut f| f.read_to_end(&mut bytes)),
                None => {
                    if io::stdin().is_terminal() {
                        eprintln!(
                            "tokencat: no input. Pipe output into it (cmd | tokencat) or use `tokencat run -- cmd`."
                        );
                        return ExitCode::from(2);
                    }
                    io::stdin().lock().read_to_end(&mut bytes)
                }
            };
            if let Err(e) = read {
                eprintln!("tokencat: {e}");
                return ExitCode::from(2);
            }
            if disabled() {
                write_out(&bytes);
                return ExitCode::SUCCESS;
            }
            let input = pipeline::Input {
                bytes: &bytes,
                exit_code: None,
                cmd: None,
            };
            let text = process_safely(&input, &cli.opts.pipeline());
            write_out(text.as_bytes());
            ExitCode::SUCCESS
        }
    }
}
