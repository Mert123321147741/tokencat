//! `tokencat run`: spawns the command under a pseudo-terminal (or plain
//! pipes), collects everything it prints, and reports its exact exit status.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub struct Captured {
    pub output: Vec<u8>,
    /// Exit code as a shell would report it: the child's code, or 128+N
    /// when it was killed by signal N.
    pub code: i32,
}

/// Errors before the child ever ran map to the shell's conventional codes.
pub struct SpawnError {
    pub code: i32,
    pub message: String,
}

const SHELL_CHARS: &[char] = &[
    ' ', '|', '&', ';', '<', '>', '(', ')', '$', '`', '"', '\'', '*', '?', '[', ']', '#', '~', '=',
    '%', '\\', '\n',
];

/// `tokencat run -- "cd web && npm test"` is run through the shell; a plain
/// argv (`tokencat run -- npm test`) is executed directly.
fn build_command(cmd: &[String]) -> Command {
    if cmd.len() == 1 && cmd[0].contains(SHELL_CHARS) {
        #[cfg(windows)]
        {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&cmd[0]);
            quiet_colors(&mut c);
            return c;
        }
        #[cfg(not(windows))]
        {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(&cmd[0]);
            quiet_colors(&mut c);
            return c;
        }
    }
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..]);
    quiet_colors(&mut c);
    c
}

/// Colors are stripped anyway, and some tools mangle them when they truncate
/// colored text, so ask for plain output unless the user chose otherwise.
fn quiet_colors(c: &mut Command) {
    let unset = |k: &str| std::env::var_os(k).is_none();
    if unset("NO_COLOR") && unset("FORCE_COLOR") {
        c.env("NO_COLOR", "1");
    }
    if unset("PY_COLORS") {
        c.env("PY_COLORS", "0");
    }
    if unset("CARGO_TERM_COLOR") {
        c.env("CARGO_TERM_COLOR", "never");
    }
}

fn spawn_error(cmd: &[String], e: io::Error) -> SpawnError {
    match e.kind() {
        io::ErrorKind::NotFound => SpawnError {
            code: 127,
            message: format!("tokencat: command not found: {}", cmd[0]),
        },
        io::ErrorKind::PermissionDenied => SpawnError {
            code: 126,
            message: format!("tokencat: permission denied: {}", cmd[0]),
        },
        _ => SpawnError {
            code: 126,
            message: format!("tokencat: cannot run {}: {e}", cmd[0]),
        },
    }
}

pub fn run(cmd: &[String], use_pty: bool) -> Result<Captured, SpawnError> {
    #[cfg(unix)]
    if use_pty {
        if let Ok(pty) = unix::Pty::open() {
            return unix::run_pty(cmd, pty);
        }
        // No PTY available (seccomp sandboxes, /dev/pts missing): fall back.
    }
    let _ = use_pty;
    run_pipes(cmd)
}

fn run_pipes(cmd: &[String]) -> Result<Captured, SpawnError> {
    let mut command = build_command(cmd);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    unix::prepare(&mut command);
    let mut child = command.spawn().map_err(|e| spawn_error(cmd, e))?;
    #[cfg(unix)]
    unix::track_child(child.id());
    let (tx, rx) = mpsc::channel();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        thread::spawn(move || pump(stream, tx));
    }
    drop(tx);
    Ok(collect(child, rx))
}

fn pump(mut src: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            // EIO on a PTY master means every slave fd is closed.
            Err(_) => break,
        }
    }
}

/// Gathers output until the child exits, then drains what is left. A
/// grandchild that keeps the terminal open (a daemon, a watcher) must not
/// make us hang, so the drain stops after a short idle period.
fn collect(mut child: Child, rx: Receiver<Vec<u8>>) -> Captured {
    let mut output = Vec::new();
    let status = loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => output.extend_from_slice(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break child.wait(),
        }
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(250)) {
        output.extend_from_slice(&chunk);
        if Instant::now() > deadline {
            break;
        }
    }
    #[cfg(unix)]
    unix::track_child(0);
    let code = match status {
        Ok(st) => exit_code(st),
        Err(_) => 1,
    };
    Captured { output, code }
}

fn exit_code(st: std::process::ExitStatus) -> i32 {
    if let Some(c) = st.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            return 128 + sig;
        }
    }
    1
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::sanitize::{SCREEN_COLS, SCREEN_ROWS};
    use std::fs::File;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::{AtomicI32, Ordering};

    static CHILD: AtomicI32 = AtomicI32::new(0);

    pub struct Pty {
        master: OwnedFd,
        slave: OwnedFd,
    }

    impl Pty {
        pub fn open() -> io::Result<Pty> {
            let mut master: libc::c_int = -1;
            let mut slave: libc::c_int = -1;
            let mut ws = libc::winsize {
                ws_row: SCREEN_ROWS as u16,
                ws_col: SCREEN_COLS as u16,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: openpty writes two fds into the provided ints; the
            // name and termios arguments may be null.
            let rc = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(ws),
                )
            };
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            for fd in [master, slave] {
                // SAFETY: fd was just returned by openpty.
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
            }
            // SAFETY: we own both fds from here on.
            Ok(unsafe {
                Pty {
                    master: OwnedFd::from_raw_fd(master),
                    slave: OwnedFd::from_raw_fd(slave),
                }
            })
        }
    }

    pub fn track_child(pid: u32) {
        CHILD.store(pid as i32, Ordering::SeqCst);
    }

    extern "C" fn forward(sig: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        let pid = CHILD.load(Ordering::SeqCst);
        if pid <= 0 {
            return;
        }
        // Ctrl-C from a terminal already reaches the child through the
        // process group; only relay signals that were sent to us directly.
        if sig == libc::SIGINT || sig == libc::SIGQUIT {
            // SAFETY: the kernel passes a valid siginfo with SA_SIGINFO.
            let code = unsafe { (*info).si_code };
            if code > 0 {
                return;
            }
        }
        // SAFETY: kill is async-signal-safe.
        unsafe { libc::kill(pid, sig) };
    }

    fn install_forwarders() {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            // SAFETY: plain sigaction setup with a handler that only calls
            // async-signal-safe functions.
            unsafe {
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = forward as *const () as usize;
                sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
                libc::sigemptyset(&mut sa.sa_mask);
                libc::sigaction(sig, &sa, std::ptr::null_mut());
            }
        }
    }

    pub fn prepare(command: &mut Command) {
        install_forwarders();
        // SAFETY: the closure only makes an async-signal-safe syscall.
        unsafe {
            command.pre_exec(|| {
                #[cfg(target_os = "linux")]
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }

    pub fn run_pty(cmd: &[String], pty: Pty) -> Result<Captured, SpawnError> {
        let Pty { master, slave } = pty;
        let mut command = build_command(cmd);
        let slave_err = slave.try_clone().map_err(|e| spawn_error(cmd, e))?;
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::from(slave))
            .stderr(Stdio::from(slave_err));
        prepare(&mut command);
        let child = command.spawn().map_err(|e| spawn_error(cmd, e))?;
        track_child(child.id());
        // Close our copies of the slave so the master sees EOF/EIO once the
        // child (and anything it spawned) is gone.
        drop(command);
        let (tx, rx) = mpsc::channel();
        let reader: Box<dyn Read + Send> = Box::new(File::from(master));
        thread::spawn(move || pump(reader, tx));
        Ok(collect(child, rx))
    }
}
