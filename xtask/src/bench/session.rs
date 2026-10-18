/* xtask/src/bench/session.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! The benchmark's private session boundary, not a general process supervisor or sandbox.
//! Process groups cover inherited children, not helpers that deliberately call `setsid`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const TERM_GRACE: Duration = Duration::from_millis(500);
const KILL_BUDGET: Duration = Duration::from_secs(2);
const BUS_STARTUP_BUDGET: Duration = Duration::from_secs(5);
const MAX_ADDRESS_BYTES: usize = 4096;
const BUS_SOCKET_NAME: &str = "b";
const SESSION_NAME_LEN: usize = 6;

static SIGNAL_SCOPE: AtomicBool = AtomicBool::new(false);
static SIGNAL_CANCELLED: AtomicBool = AtomicBool::new(false);

extern "C" fn cancel_signal(_: libc::c_int) {
    // The handler does no allocation, logging, locking, or process cleanup.
    SIGNAL_CANCELLED.store(true, Ordering::Relaxed);
}

struct SignalHandlers {
    interrupt: libc::sigaction,
    terminate: libc::sigaction,
}

/// A default token is entirely independent of the process-wide installed signal scope.
#[derive(Default)]
pub(super) struct Cancellation {
    requested: AtomicBool,
    handlers: Option<SignalHandlers>,
}

impl Cancellation {
    pub(super) fn install() -> Result<Self, String> {
        SIGNAL_SCOPE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| "benchmark cancellation handlers are already installed".to_owned())?;
        SIGNAL_CANCELLED.store(false, Ordering::Relaxed);
        // SAFETY: zero is a valid initial representation for sigaction and its signal mask.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = cancel_signal as *const () as usize;
        // SAFETY: action owns a valid, writable signal mask.
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        // SAFETY: these are writable storage for the previous signal actions.
        let mut interrupt: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let mut terminate: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers refer to initialized sigaction storage; the handler is atomic-only.
        if unsafe { libc::sigaction(libc::SIGINT, &action, &mut interrupt) } != 0 {
            let error = io::Error::last_os_error();
            SIGNAL_SCOPE.store(false, Ordering::Release);
            return Err(format!("install benchmark SIGINT handler: {error}"));
        }
        // SAFETY: as above, now for SIGTERM.
        if unsafe { libc::sigaction(libc::SIGTERM, &action, &mut terminate) } != 0 {
            let error = io::Error::last_os_error();
            // SAFETY: interrupt was filled by the successful SIGINT sigaction call.
            let restored =
                unsafe { libc::sigaction(libc::SIGINT, &interrupt, std::ptr::null_mut()) };
            SIGNAL_SCOPE.store(false, Ordering::Release);
            return Err(if restored == 0 {
                format!("install benchmark SIGTERM handler: {error}")
            } else {
                format!(
                    "install benchmark SIGTERM handler: {error}; restore SIGINT: {}",
                    io::Error::last_os_error()
                )
            });
        }
        Ok(Self {
            requested: AtomicBool::new(false),
            handlers: Some(SignalHandlers {
                interrupt,
                terminate,
            }),
        })
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
            || (self.handlers.is_some() && SIGNAL_CANCELLED.load(Ordering::Relaxed))
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("benchmark cancelled".into())
        } else {
            Ok(())
        }
    }

    pub(super) fn wait(&self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now()
            .checked_add(duration)
            .ok_or("benchmark wait duration is too large")?;
        loop {
            self.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            sleep(remaining.min(POLL_INTERVAL));
        }
    }

    #[cfg(test)]
    pub(super) fn request(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        let Some(handlers) = self.handlers.take() else {
            return;
        };
        for (signal, previous) in [
            (libc::SIGINT, &handlers.interrupt),
            (libc::SIGTERM, &handlers.terminate),
        ] {
            // SAFETY: previous is the original action returned by sigaction for this signal.
            if unsafe { libc::sigaction(signal, previous, std::ptr::null_mut()) } != 0 {
                let _ = writeln!(
                    io::stderr(),
                    "[bench] failed to restore signal {signal}: {}",
                    io::Error::last_os_error()
                );
            }
        }
        SIGNAL_SCOPE.store(false, Ordering::Release);
    }
}

/// The unreaped leader anchors the numeric process-group identity until the last group signal.
pub(super) struct ManagedChild {
    child: Child,
    reaped: Option<ExitStatus>,
    #[cfg(test)]
    signals_sent: std::cell::Cell<usize>,
}

impl ManagedChild {
    pub(super) fn spawn(command: &mut Command) -> Result<Self, String> {
        let child = command
            .process_group(0)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", command.get_program().to_string_lossy()))?;
        Ok(Self {
            child,
            reaped: None,
            #[cfg(test)]
            signals_sent: std::cell::Cell::new(0),
        })
    }

    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    /// Observe terminal status without reaping: `Child::try_wait` is unsafe for our PGID anchor.
    pub(super) fn status(&self) -> Result<Option<ExitStatus>, String> {
        if let Some(status) = self.reaped {
            return Ok(Some(status));
        }
        loop {
            // SAFETY: a zeroed siginfo_t is valid output storage and makes the no-event PID zero.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: we own this child and request only non-reaping, nonblocking exit observation.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
                )
            };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!(
                    "observe child {} without reaping: {error}",
                    self.id()
                ));
            }
            // SAFETY: waitid initialized the SIGCHLD fields in info, or left the zero PID intact.
            if unsafe { info.si_pid() } == 0 {
                return Ok(None);
            }
            // SAFETY: this is a terminal SIGCHLD event from waitid(WEXITED).
            let code = unsafe { info.si_status() };
            let raw = match info.si_code {
                libc::CLD_EXITED => code << 8,
                libc::CLD_KILLED => code & 0x7f,
                libc::CLD_DUMPED => (code & 0x7f) | 0x80,
                other => return Err(format!("unexpected child exit event {other}")),
            };
            return Ok(Some(ExitStatus::from_raw(raw)));
        }
    }

    fn signal_group(&self, signal: libc::c_int) -> Result<(), String> {
        if self.reaped.is_some() {
            return Err("refusing to signal a process group after reaping its leader".into());
        }
        // ECHILD also prevents signalling if an unexpected external reaper consumed the leader.
        self.status()?;
        let pgid = libc::pid_t::try_from(self.id()).map_err(|_| "child PID exceeds pid_t")?;
        #[cfg(test)]
        self.signals_sent.set(self.signals_sent.get() + 1);
        // SAFETY: process_group(0) established this group, and its leader remains our unreaped child.
        if unsafe { libc::kill(-pgid, signal) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(format!("signal owned process group {pgid}: {error}"));
            }
        }
        Ok(())
    }

    fn wait_quiet(&self, deadline: Instant) -> Result<bool, String> {
        loop {
            if !group_has_live_members(self.id())? {
                return Ok(true);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            sleep(remaining.min(POLL_INTERVAL));
        }
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        if self.reaped.is_some() {
            return Ok(());
        }
        self.signal_group(libc::SIGTERM)?;
        // Failure to establish quiescence is also a reason to escalate, never to reap early.
        let _ = self.wait_quiet(Instant::now() + TERM_GRACE);
        let deadline = Instant::now() + KILL_BUDGET;
        // Close the /proc scan/fork race even when TERM appeared to stop the whole group.
        // The unreaped leader still anchors this final signal; already-exited tasks are unaffected.
        self.signal_group(libc::SIGKILL)?;
        if !self.wait_quiet(deadline)? {
            return Err(format!(
                "owned process group {} did not become quiescent after SIGKILL; leader retained",
                self.id()
            ));
        }
        // No group signals are allowed beyond this point. A kernel D-state task can still prevent
        // successful cleanup; the observation budget is finite, not a promise of mandatory reaping.
        loop {
            if self.status()?.is_some() {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        self.reaped = Some(status);
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(e) => return Err(format!("reap child {}: {e}", self.id())),
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "child {} was not reapable within cleanup budget",
                    self.id()
                ));
            }
            sleep(remaining.min(POLL_INTERVAL));
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            let _ = writeln!(io::stderr(), "[bench] process cleanup failed: {error}");
        }
    }
}

fn proc_group_is_live(stat: &[u8], group: u32) -> Option<bool> {
    let close = stat.iter().rposition(|byte| *byte == b')')?;
    let mut fields = stat.get(close + 1..)?.split(u8::is_ascii_whitespace);
    let mut fields = fields.by_ref().filter(|field| !field.is_empty());
    let state = *fields.next()?.first()?;
    let pgid: u32 = std::str::from_utf8(fields.nth(1)?).ok()?.parse().ok()?;
    // Field 20: even a zombie leader can retain live (including D-state) worker threads.
    let threads: u32 = std::str::from_utf8(fields.nth(14)?).ok()?.parse().ok()?;
    Some(pgid == group && (!matches!(state, b'Z' | b'X' | b'x') || threads > 1))
}

fn group_has_live_members(group: u32) -> Result<bool, String> {
    for entry in fs::read_dir("/proc").map_err(|e| format!("inspect owned process group: {e}"))? {
        let entry = entry.map_err(|e| format!("inspect /proc entry: {e}"))?;
        if entry.file_name().as_bytes().is_empty()
            || !entry.file_name().as_bytes().iter().all(u8::is_ascii_digit)
        {
            continue;
        }
        let stat = match fs::read(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(e)
                if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ESRCH) =>
            {
                continue;
            }
            Err(e) => return Err(format!("inspect {}: {e}", entry.path().display())),
        };
        let live = proc_group_is_live(&stat, group)
            .ok_or_else(|| format!("malformed process stat at {}", entry.path().display()))?;
        if live {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) struct PrivateSession {
    directory: TempDir,
    home: PathBuf,
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    state: PathBuf,
    runtime: PathBuf,
    address: String,
    bus: Option<ManagedChild>,
    preserved: bool,
    retired: bool,
}

impl PrivateSession {
    fn allocate() -> Result<Self, String> {
        let base = crate::root().join("target/isolated");
        fs::create_dir_all(&base).map_err(|e| format!("create {}: {e}", base.display()))?;
        let base =
            fs::canonicalize(&base).map_err(|e| format!("resolve private session root: {e}"))?;
        let directory = tempfile::Builder::new()
            .prefix("")
            .rand_bytes(SESSION_NAME_LEN)
            .tempdir_in(base)
            .map_err(|e| format!("create private session directory: {e}"))?;
        Self::from_directory(directory)
    }

    fn from_directory(directory: TempDir) -> Result<Self, String> {
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("protect private session root: {e}"))?;
        validate_private_directory(directory.path())?;
        let make_dir = |name: &str| -> Result<PathBuf, String> {
            let path = directory.path().join(name);
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .map_err(|e| format!("create {}: {e}", path.display()))?;
            Ok(path)
        };
        let home = make_dir("home")?;
        let config = make_dir("config")?;
        let data = make_dir("data")?;
        let cache = make_dir("cache")?;
        let state = make_dir("state")?;
        // The owned root already meets XDG runtime requirements. Avoid another path component:
        // D-Bus uses a smaller portable limit than Linux's sockaddr_un.
        let runtime = directory.path().to_path_buf();
        validate_socket_path(&runtime.join(super::SOCKET_NAME))?;
        let address = bus_address(&runtime.join(BUS_SOCKET_NAME))?;
        Ok(Self {
            directory,
            home,
            config,
            data,
            cache,
            state,
            runtime,
            address,
            bus: None,
            preserved: false,
            retired: false,
        })
    }

    pub(super) fn new(cancellation: &Cancellation) -> Result<Self, String> {
        cancellation.check()?;
        let mut session = Self::allocate()?;
        // Prefer the system daemon to a potentially incompatible SDK/Conda version on PATH.
        let daemon = if Path::new("/usr/bin/dbus-daemon").is_file() {
            "/usr/bin/dbus-daemon"
        } else {
            "dbus-daemon"
        };
        session.start_bus(cancellation, &mut Command::new(daemon), BUS_STARTUP_BUDGET)?;
        Ok(session)
    }

    fn start_bus(
        &mut self,
        cancellation: &Cancellation,
        command: &mut Command,
        budget: Duration,
    ) -> Result<(), String> {
        cancellation.check()?;
        let address_file = self.root().join("bus-address");
        let stdout = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&address_file)
            .map_err(|e| format!("create private bus address file: {e}"))?;
        // Separate open descriptions avoid sharing the daemon's write offset. A regular private
        // file cannot block awaiting a newline; O_NONBLOCK also makes that requirement explicit.
        let mut reader = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&address_file)
            .map_err(|e| format!("open private bus address file: {e}"))?;
        self.apply(command);
        command
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .arg(format!("--address={}", self.address))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::inherit());
        self.bus = Some(ManagedChild::spawn(command)?);
        self.wait_for_bus(cancellation, &mut reader, Instant::now() + budget)
    }

    fn wait_for_bus(
        &self,
        cancellation: &Cancellation,
        reader: &mut File,
        deadline: Instant,
    ) -> Result<(), String> {
        let bus = self.bus.as_ref().ok_or("private bus was not spawned")?;
        let mut output = Vec::new();
        let mut buffer = [0; 512];
        loop {
            cancellation.check()?;
            if let Some(status) = bus.status()? {
                return Err(format!(
                    "private dbus-daemon exited during startup: {status}"
                ));
            }
            match reader.read(&mut buffer) {
                Ok(size) => output.extend_from_slice(&buffer[..size]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(format!("read private bus readiness: {e}")),
            }
            if output.len() > MAX_ADDRESS_BYTES {
                return Err("private dbus-daemon printed an oversized address".into());
            }
            if let Some(newline) = output.iter().position(|byte| *byte == b'\n') {
                let printed = std::str::from_utf8(&output[..newline])
                    .map_err(|_| "private dbus-daemon printed a non-UTF-8 address")?;
                if !matches_bus_address(printed, &self.address) {
                    return Err(format!(
                        "private dbus-daemon did not bind the requested private address: expected {:?}, printed {printed:?}",
                        self.address
                    ));
                }
                let socket = self.runtime.join(BUS_SOCKET_NAME);
                if fs::symlink_metadata(&socket)
                    .is_ok_and(|metadata| metadata.file_type().is_socket())
                    && bus.status()?.is_none()
                {
                    return Ok(());
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(
                    "private dbus-daemon readiness timed out; no session-bus fallback".into(),
                );
            }
            cancellation.wait(remaining.min(POLL_INTERVAL))?;
        }
    }

    pub(super) fn apply(&self, command: &mut Command) {
        command
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_STATE_HOME", &self.state)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_DIRS", "/etc/xdg")
            .env("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address);
        for key in [
            "DBUS_SESSION_BUS_PID",
            "DBUS_SESSION_BUS_WINDOWID",
            "DBUS_STARTER_ADDRESS",
            "DBUS_STARTER_BUS_TYPE",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "WAYLAND_SOCKET",
            "XAUTHORITY",
            "SESSION_MANAGER",
            "MC_DEBUG_MAGPIE_PROCESS_SOCK",
            "MC_MAGPIE_CONFIG_DIR",
        ] {
            command.env_remove(key);
        }
    }

    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn config_home(&self) -> &Path {
        &self.config
    }

    pub(super) fn runtime_dir(&self) -> &Path {
        &self.runtime
    }

    pub(super) fn preserve(&mut self) {
        self.directory.disable_cleanup(true);
        if !self.preserved {
            self.preserved = true;
            let _ = writeln!(
                io::stderr(),
                "[bench] retaining private session directory {}",
                self.root().display()
            );
        }
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        if let Some(bus) = self.bus.as_mut()
            && let Err(error) = bus.shutdown()
        {
            self.preserve();
            return Err(format!("private session bus cleanup failed: {error}"));
        }
        self.bus.take();
        if !self.preserved && !self.retired {
            if let Err(error) = fs::remove_dir_all(self.root()) {
                self.preserve();
                return Err(format!(
                    "remove private session directory {}: {error}",
                    self.root().display()
                ));
            }
            self.directory.disable_cleanup(true);
            self.retired = true;
        }
        Ok(())
    }
}

impl Drop for PrivateSession {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            let _ = writeln!(io::stderr(), "[bench] {error}");
        }
    }
}

fn validate_private_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("inspect private directory {}: {e}", path.display()))?;
    // SAFETY: geteuid has no arguments or memory-safety preconditions.
    let owner = unsafe { libc::geteuid() };
    if !path.is_absolute()
        || !metadata.file_type().is_dir()
        || metadata.uid() != owner
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(format!(
            "private directory must be absolute, owned, nonsymlink, and mode 0700: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_socket_path(socket: &Path) -> Result<(), String> {
    let bytes = socket.as_os_str().as_bytes();
    // SAFETY: sockaddr_un consists of integer/byte fields, for which zero is valid.
    let capacity = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
        .sun_path
        .len();
    if !socket.is_absolute() || bytes.contains(&0) || bytes.len() + 1 > capacity {
        return Err(format!(
            "private socket needs an absolute, NUL-free path of at most {capacity} bytes including its terminator (got {}): {}",
            bytes.len() + 1,
            socket.display()
        ));
    }
    Ok(())
}

fn bus_address(socket: &Path) -> Result<String, String> {
    validate_socket_path(socket)?;
    // D-Bus caps pathname sockets at 99 bytes even on Linux (excluding NUL).
    if socket.as_os_str().len() > 99 {
        return Err(format!(
            "private bus socket exceeds the portable D-Bus limit of 99 bytes: {}",
            socket.display()
        ));
    }
    let bytes = socket.as_os_str().as_bytes();
    let mut escaped = String::new();
    // Match dbus-daemon's canonical escaping in its printed readiness address.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/' | b'\\' | b'*' | b'.')
        {
            escaped.push(char::from(*byte));
        } else {
            escaped.push('%');
            escaped.push(char::from(HEX[usize::from(byte >> 4)]));
            escaped.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    Ok(format!("unix:path={escaped}"))
}

fn matches_bus_address(printed: &str, expected: &str) -> bool {
    printed == expected
        || printed
            .strip_prefix(expected)
            .and_then(|rest| rest.strip_prefix(",guid="))
            .is_some_and(|guid| {
                guid.len() == 32 && guid.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "test condition timed out");
            sleep(POLL_INTERVAL);
        }
    }

    #[test]
    fn environment_overlay_replaces_poison_and_preserves_unrelated_settings() {
        let session = PrivateSession::allocate().unwrap();
        let mut command = Command::new("unused-test-command");
        let replaced = [
            ("HOME", session.home.as_os_str()),
            ("XDG_CONFIG_HOME", session.config_home().as_os_str()),
            ("XDG_DATA_HOME", session.data.as_os_str()),
            ("XDG_CACHE_HOME", session.cache.as_os_str()),
            ("XDG_STATE_HOME", session.state.as_os_str()),
            ("XDG_RUNTIME_DIR", session.runtime_dir().as_os_str()),
            ("XDG_CONFIG_DIRS", OsStr::new("/etc/xdg")),
            ("XDG_DATA_DIRS", OsStr::new("/usr/local/share:/usr/share")),
            ("DBUS_SESSION_BUS_ADDRESS", OsStr::new(&session.address)),
        ];
        let removed = [
            "DBUS_SESSION_BUS_PID",
            "DBUS_SESSION_BUS_WINDOWID",
            "DBUS_STARTER_ADDRESS",
            "DBUS_STARTER_BUS_TYPE",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "WAYLAND_SOCKET",
            "XAUTHORITY",
            "SESSION_MANAGER",
            "MC_DEBUG_MAGPIE_PROCESS_SOCK",
            "MC_MAGPIE_CONFIG_DIR",
        ];
        let preserved = [
            "PATH",
            "RUST_LOG",
            "LANG",
            "DBUS_SYSTEM_BUS_ADDRESS",
            "DRI_PRIME",
            "QT_LOGGING_RULES",
            "KDE_FULL_SESSION",
        ];
        for (key, _) in replaced {
            command.env(key, "host-poison");
        }
        for key in removed {
            command.env(key, "host-poison");
        }
        for key in preserved {
            command.env(key, "preserved");
        }
        session.apply(&mut command);
        let overlay: BTreeMap<_, _> = command.get_envs().collect();
        for (key, value) in replaced {
            assert_eq!(overlay.get(OsStr::new(key)), Some(&Some(value)), "{key}");
        }
        for key in removed {
            assert_eq!(overlay.get(OsStr::new(key)), Some(&None), "{key}");
        }
        for key in preserved {
            assert_eq!(
                overlay.get(OsStr::new(key)),
                Some(&Some(OsStr::new("preserved")))
            );
        }
    }

    #[test]
    fn private_directories_are_unique_owned_and_removed_unless_preserved() {
        let first = PrivateSession::allocate().unwrap();
        let mut second = PrivateSession::allocate().unwrap();
        assert_ne!(first.root(), second.root());
        for directory in [
            first.root(),
            first.runtime_dir(),
            second.root(),
            second.runtime_dir(),
        ] {
            validate_private_directory(directory).unwrap();
        }
        let removed = first.root().to_path_buf();
        drop(first);
        assert!(!removed.exists());
        let retained = second.root().to_path_buf();
        second.preserve();
        drop(second);
        assert!(retained.is_dir());
        fs::remove_dir_all(retained).unwrap();
    }

    #[test]
    fn addresses_escape_filesystem_bytes_and_count_the_socket_terminator() {
        let socket = Path::new(OsStr::from_bytes(b"/private/a b,%;=\\*\xff/bus"));
        assert_eq!(
            bus_address(socket).unwrap(),
            "unix:path=/private/a%20b%2c%25%3b%3d\\*%ff/bus"
        );
        assert!(bus_address(Path::new("relative/bus")).is_err());
        assert!(bus_address(Path::new(OsStr::from_bytes(b"/bad\0socket"))).is_err());
        // D-Bus uses a portable limit of 99 pathname bytes, excluding NUL.
        assert!(bus_address(Path::new(&format!("/{}", "a".repeat(98)))).is_ok());
        assert!(bus_address(Path::new(&format!("/{}", "a".repeat(99)))).is_err());
        // Linux sockaddr_un.sun_path separately has 108 bytes, including NUL.
        for socket in [BUS_SOCKET_NAME, super::super::SOCKET_NAME] {
            let runtime = format!("/{}", "a".repeat(105 - socket.len()));
            assert!(validate_socket_path(&Path::new(&runtime).join(socket)).is_ok());
            assert!(validate_socket_path(&Path::new(&format!("{runtime}a")).join(socket)).is_err());
        }
        assert!(matches_bus_address(
            "unix:path=/bus,guid=0123456789abcdef0123456789abcdef",
            "unix:path=/bus"
        ));
        assert!(!matches_bus_address(
            "unix:path=/bus-other",
            "unix:path=/bus"
        ));
        assert!(!matches_bus_address(
            "unix:path=/bus,guid=bad",
            "unix:path=/bus"
        ));
    }

    #[test]
    fn short_socket_paths_fit_the_standard_ci_checkout() {
        let checkout =
            Path::new("/home/runner/work/win10-task-manager-on-linux/win10-task-manager-on-linux");
        let runtime = checkout
            .join("target/isolated")
            .join("A".repeat(SESSION_NAME_LEN));
        bus_address(&runtime.join(BUS_SOCKET_NAME)).unwrap();
        validate_socket_path(&runtime.join(super::super::SOCKET_NAME)).unwrap();
    }

    fn private_bus_id(session: &PrivateSession) -> String {
        let mut command = Command::new("/usr/bin/dbus-send");
        session.apply(&mut command);
        let output = command
            .arg(format!("--bus={}", session.address))
            .args([
                "--dest=org.freedesktop.DBus",
                "--type=method_call",
                "--print-reply=literal",
                "--reply-timeout=2000",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus.GetId",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let id = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        assert_eq!(id.len(), 32, "unexpected private bus ID {id:?}");
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        id
    }

    #[test]
    fn real_private_buses_have_distinct_ids_and_stop_before_directory_release() {
        let cancellation = Cancellation::default();
        let mut first = PrivateSession::new(&cancellation).unwrap();
        let mut second = PrivateSession::new(&cancellation).unwrap();
        assert_ne!(private_bus_id(&first), private_bus_id(&second));
        let root = first.root().to_path_buf();
        let leader = first.bus.as_ref().unwrap().id();
        first.shutdown().unwrap();
        first.shutdown().unwrap();
        assert!(!Path::new(&format!("/proc/{leader}")).exists());
        assert!(
            !root.exists(),
            "explicit shutdown must release the directory last"
        );
        drop(first);
        assert!(!root.exists());
        second.shutdown().unwrap();
    }

    #[test]
    fn real_bus_accepts_percent_escaped_runtime_paths() {
        // Keep the root's length unchanged even when testing escaping on a long checkout.
        let base = crate::root().join("target/isolated");
        fs::create_dir_all(&base).unwrap();
        let prefix = " ,;%";
        let directory = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(SESSION_NAME_LEN - prefix.len())
            .tempdir_in(base)
            .unwrap();
        let mut session = PrivateSession::from_directory(directory).unwrap();
        session
            .start_bus(
                &Cancellation::default(),
                &mut Command::new("/usr/bin/dbus-daemon"),
                BUS_STARTUP_BUDGET,
            )
            .unwrap();
        private_bus_id(&session);
        session.shutdown().unwrap();
    }

    #[test]
    fn zombie_leaders_with_worker_threads_are_not_quiescent() {
        let stat = |state, count| {
            format!(
                "1 (helper with ) in name) {state} 0 42 {}{count}",
                "0 ".repeat(14)
            )
        };
        assert_eq!(
            proc_group_is_live(stat("Z", "1").as_bytes(), 42),
            Some(false)
        );
        assert_eq!(
            proc_group_is_live(stat("Z", "2").as_bytes(), 42),
            Some(true)
        );
        assert_eq!(
            proc_group_is_live(stat("D", "1").as_bytes(), 42),
            Some(true)
        );
        assert_eq!(
            proc_group_is_live(stat("S", "2").as_bytes(), 43),
            Some(false)
        );
        for count in ["", "invalid"] {
            assert_eq!(proc_group_is_live(stat("Z", count).as_bytes(), 42), None);
        }
    }

    #[test]
    fn failed_missing_and_silent_bus_startups_do_not_fall_back() {
        let cancellation = Cancellation::default();
        let cases = ["missing", "failed", "silent", "wrong-address"];
        for case in cases {
            let mut session = PrivateSession::allocate().unwrap();
            let root = session.root().to_path_buf();
            let mut command = if case == "missing" {
                Command::new(session.root().join("nonexistent-dbus-daemon"))
            } else {
                let mut command = Command::new("/bin/sh");
                command.args([
                    "-c",
                    match case {
                        "failed" => "exit 23",
                        "silent" => "exec sleep 60",
                        _ => "printf 'unix:path=/not-the-private-bus\\n'; exec sleep 60",
                    },
                ]);
                command
            };
            // A poisoned command-level value must not leak even to the daemon itself.
            command.env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/not-a-real-host-bus");
            let error = session
                .start_bus(&cancellation, &mut command, Duration::from_millis(100))
                .unwrap_err();
            assert!(
                error.contains("spawn") || error.contains("startup") || error.contains("private")
            );
            assert_eq!(
                command
                    .get_envs()
                    .find(|(key, _)| *key == "DBUS_SESSION_BUS_ADDRESS")
                    .unwrap()
                    .1,
                Some(OsStr::new(&session.address))
            );
            drop(session);
            assert!(!root.exists(), "failed startup directory leaked for {case}");
        }
    }

    #[test]
    fn owned_groups_clean_exited_leaders_and_descendants_without_touching_sentinel() {
        let mut sentinel_command = Command::new("/bin/sh");
        sentinel_command.args(["-c", "exec sleep 60"]);
        let mut sentinel = ManagedChild::spawn(&mut sentinel_command).unwrap();
        for ignore_term in [false, true] {
            let session = PrivateSession::allocate().unwrap();
            let ready = session.root().join("ready");
            let pid_file = session.root().join("descendant");
            let script = format!(
                "(trap {} TERM; : > \"$1\"; exec sleep 60) & printf '%s\\n' \"$!\" > \"$2\"; exit 0",
                if ignore_term { "''" } else { "-" }
            );
            let mut command = Command::new("/bin/sh");
            session.apply(&mut command);
            command
                .args(["-c", &script, "owned-group-test"])
                .arg(&ready)
                .arg(&pid_file);
            let mut child = ManagedChild::spawn(&mut command).unwrap();
            wait_until(|| ready.exists() && pid_file.exists() && child.status().unwrap().is_some());
            let descendant: u32 = fs::read_to_string(pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(group_has_live_members(child.id()).unwrap());
            assert!(child.status().unwrap().unwrap().success());
            assert!(
                child.status().unwrap().unwrap().success(),
                "status must not reap"
            );
            child.shutdown().unwrap();
            assert!(!group_has_live_members(child.id()).unwrap());
            let stat = fs::read(format!("/proc/{descendant}/stat"));
            assert!(
                stat.is_err()
                    || stat.is_ok_and(|stat| {
                        proc_group_is_live(&stat, child.id()).is_some_and(|live| !live)
                    })
            );
            let signals = child.signals_sent.get();
            assert_eq!(signals, 2);
            child.shutdown().unwrap();
            assert!(child.signal_group(libc::SIGKILL).is_err());
            assert_eq!(child.signals_sent.get(), signals, "no signal after reaping");
            assert!(sentinel.status().unwrap().is_none());
        }
        sentinel.shutdown().unwrap();
    }

    #[test]
    fn cooperative_group_child() {
        let Some(root) = std::env::var_os("MC_BENCH_COOPERATIVE_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let role = std::env::var("MC_BENCH_COOPERATIVE_ROLE").unwrap();
        assert!(matches!(role.as_str(), "parent" | "child"));
        let cancellation = Cancellation::install().unwrap();
        let mut child = (role == "parent").then(|| {
            // Deliberately inherit the managed parent's process group.
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "bench::session::tests::cooperative_group_child"])
                .env("MC_BENCH_COOPERATIVE_ROLE", "child")
                .spawn()
                .unwrap()
        });
        fs::write(root.join(format!("{role}.ready")), b"").unwrap();
        while !cancellation.is_cancelled() {
            sleep(POLL_INTERVAL);
        }
        if let Some(child) = child.as_mut() {
            assert!(child.wait().unwrap().success());
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("markers"))
            .unwrap()
            .write_all(role.as_bytes())
            .unwrap();
    }

    #[test]
    fn cooperative_group_gets_term_grace_before_final_kill() {
        let session = PrivateSession::allocate().unwrap();
        let markers = session.root().join("markers");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "bench::session::tests::cooperative_group_child"])
            .env("MC_BENCH_COOPERATIVE_ROOT", session.root())
            .env("MC_BENCH_COOPERATIVE_ROLE", "parent")
            .stdout(Stdio::null());
        let mut child = ManagedChild::spawn(&mut command).unwrap();
        wait_until(|| {
            session.root().join("child.ready").exists()
                && session.root().join("parent.ready").exists()
        });
        child.shutdown().unwrap();
        let status = child.status().unwrap().unwrap();
        let markers = fs::read_to_string(markers);
        assert!(status.success(), "status {status}; markers {markers:?}");
        assert_eq!(markers.unwrap(), "childparent");
        assert!(!Path::new(&format!("/proc/{}", child.id())).exists());
    }

    #[test]
    fn cancellation_wait_preserves_deadlines_and_tokens_are_independent() {
        let token = Cancellation::default();
        let other = Cancellation::default();
        let start = Instant::now();
        token.wait(Duration::from_millis(45)).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(45));
        std::thread::scope(|scope| {
            scope.spawn(|| {
                sleep(Duration::from_millis(30));
                token.request();
            });
            let start = Instant::now();
            assert!(token.wait(Duration::from_secs(3)).is_err());
            assert!(start.elapsed() < Duration::from_secs(1));
        });
        assert!(token.is_cancelled());
        assert!(!other.is_cancelled());
        other.wait(Duration::ZERO).unwrap();
    }

    static RESTORED_HANDLER_CALLED: AtomicBool = AtomicBool::new(false);

    extern "C" fn restored_signal(_: libc::c_int) {
        RESTORED_HANDLER_CALLED.store(true, Ordering::Relaxed);
    }

    #[test]
    fn installed_cancellation_child() {
        let Some(signal) = std::env::var_os("MC_BENCH_SIGNAL_TEST_CHILD") else {
            return;
        };
        let signal = if signal == "SIGINT" {
            libc::SIGINT
        } else {
            libc::SIGTERM
        };
        // Only executed by a dedicated subprocess, never the main test runner.
        // SAFETY: writable sigaction output storage and an atomic-only test handler.
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: sigaction is initialized before installation.
        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        original.sa_sigaction = restored_signal as *const () as usize;
        // SAFETY: valid signal/action pointers; this subprocess owns its handlers.
        assert_eq!(
            unsafe { libc::sigaction(signal, &original, &mut previous) },
            0
        );
        let token = Cancellation::install().unwrap();
        let independent = Cancellation::default();
        assert!(Cancellation::install().is_err());
        // SAFETY: signal only this subprocess's own PID, never the host/test-runner group.
        assert_eq!(unsafe { libc::kill(libc::getpid(), signal) }, 0);
        wait_until(|| token.is_cancelled());
        assert!(token.wait(Duration::ZERO).is_err());
        assert!(!independent.is_cancelled());
        drop(token);
        // SAFETY: the previous atomic-only test handler must have been restored.
        assert_eq!(unsafe { libc::kill(libc::getpid(), signal) }, 0);
        wait_until(|| RESTORED_HANDLER_CALLED.load(Ordering::Relaxed));
        // SAFETY: restore the subprocess's initial action before returning to its test harness.
        assert_eq!(
            unsafe { libc::sigaction(signal, &previous, std::ptr::null_mut()) },
            0
        );
    }

    #[test]
    fn real_signals_are_scoped_to_isolated_test_subprocesses() {
        for signal in ["SIGINT", "SIGTERM"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "bench::session::tests::installed_cancellation_child",
                    "--nocapture",
                ])
                .env("MC_BENCH_SIGNAL_TEST_CHILD", OsString::from(signal))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit());
            let mut child = ManagedChild::spawn(&mut command).unwrap();
            wait_until(|| child.status().unwrap().is_some());
            let status = child.status().unwrap().unwrap();
            child.shutdown().unwrap();
            assert!(
                status.success(),
                "isolated {signal} handler test failed: {status}"
            );
        }
    }
}
