/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! Private helper transport and child ownership. Used only by the hardware worker.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const HELPER: &str = include_str!("../../../support/taskmanager-hardware");
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(3);

fn helper_code() -> String {
    let mut code = String::new();
    for (name, text) in [
        ("INSTALL_SOURCE", HELPER),
        (
            "SERVICE_TEXT",
            include_str!("../../../data/taskmanager-hardware.service"),
        ),
        (
            "SOCKET_TEXT",
            include_str!("../../../data/taskmanager-hardware.socket"),
        ),
        (
            "POLICY_TEXT",
            include_str!("../../../data/io.github.taskmanager.hardware.policy"),
        ),
    ] {
        // JSON string literals are valid Python strings for these source payloads.
        code.push_str(&format!(
            "{name} = {}\n",
            serde_json::to_string(text).unwrap()
        ));
    }
    code.push_str(HELPER);
    code
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardwareTelemetry {
    pub cpu_voltage_v: Option<f32>,
    pub ram_voltage_v: Option<f32>,
    pub cpu_multiplier: Option<f32>,
    pub bclk_mhz: Option<f32>,
    pub fclk_mhz: Option<f32>,
    pub mclk_mhz: Option<f32>,
    pub uclk_mhz: Option<f32>,
    pub product_serial: Option<String>,
    pub board_serial: Option<String>,
    pub product_uuid: Option<String>,
    pub intel_gear: Option<u32>,
    pub multiplier_source: Option<String>,
    pub clock_source: Option<String>,
    pub bclk_source: Option<String>,
    pub gear_source: Option<String>,
    pub voltage_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareMode {
    Off,
    Temporary,
    Persistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareGrant {
    Absent,
    Present,
    Unconfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareStatus {
    pub epoch: u64,
    pub mode: HardwareMode,
    pub sampling: bool,
    pub grant: HardwareGrant,
}

impl Default for HardwareStatus {
    fn default() -> Self {
        Self {
            epoch: 0,
            mode: HardwareMode::Off,
            sampling: false,
            grant: HardwareGrant::Unconfirmed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareOperation {
    Enable { interactive: bool },
    Disable,
    Install,
    Spd,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Reply {
    pub enabled: bool,
    pub data: Option<HardwareTelemetry>,
    pub error: Option<String>,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub revoked: bool,
}

/// Cancellation is out-of-band, including while a socket is waiting for readiness.
pub(crate) struct Control {
    pub running: Arc<AtomicBool>,
    pub cancelled: AtomicU64,
    pub active: AtomicU64,
    pub epoch: AtomicU64,
    pub sampling: AtomicBool,
}

impl Control {
    pub fn new(running: Arc<AtomicBool>) -> Self {
        Self {
            running,
            cancelled: AtomicU64::new(0),
            active: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            sampling: AtomicBool::new(false),
        }
    }
    pub fn check(&self, id: u64, deadline: Instant) -> Result<(), String> {
        if !self.running.load(Ordering::Acquire) {
            Err("Task Manager is closing.".into())
        } else if id != 0 && self.cancelled.load(Ordering::Acquire) == id {
            Err("Hardware operation cancelled.".into())
        } else if Instant::now() >= deadline {
            Err("The hardware operation timed out.".into())
        } else {
            Ok(())
        }
    }
    pub fn invalidate(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn sampling_interrupted(&self) -> bool {
        let cancelled = self.cancelled.load(Ordering::Acquire);
        !self.sampling.load(Ordering::Acquire)
            || (cancelled != 0 && self.active.load(Ordering::Acquire) == cancelled)
    }
    fn check_io(&self, id: Option<u64>, deadline: Instant) -> Result<(), String> {
        self.check(id.unwrap_or(0), deadline)?;
        if id.is_none() && self.sampling_interrupted() {
            Err("Sampling interrupted for hardware control.".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn call_json<T: serde::de::DeserializeOwned>(
    socket: &Path,
    request: &str,
    limit: usize,
    control: &Control,
    id: Option<u64>,
    deadline: Instant,
) -> Result<T, String> {
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::{ffi::OsStrExt, net::UnixStream};

    fn ready(
        fd: libc::c_int,
        events: libc::c_short,
        control: &Control,
        id: Option<u64>,
        deadline: Instant,
    ) -> Result<(), String> {
        loop {
            control.check_io(id, deadline)?;
            let mut descriptor = libc::pollfd {
                fd,
                events,
                revents: 0,
            };
            // A short readiness slice observes cancellation; the absolute deadline never resets.
            let result = unsafe { libc::poll(&mut descriptor, 1, 20) };
            if result > 0 {
                return Ok(());
            }
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
    }

    control.check_io(id, deadline)?;
    let bytes = socket.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err("Invalid hardware socket path.".into());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (to, from) in address.sun_path.iter_mut().zip(bytes) {
        *to = *from as libc::c_char;
    }
    // Immediately transfer the newly owned descriptor to RAII; all paths close it.
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    loop {
        control.check_io(id, deadline)?;
        let result = unsafe {
            libc::connect(
                fd,
                &address as *const _ as *const libc::sockaddr,
                (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t,
            )
        };
        if result == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINPROGRESS) => {
                ready(fd, libc::POLLOUT, control, id, deadline)?;
                if let Some(error) = stream.take_error().map_err(|e| e.to_string())? {
                    return Err(error.to_string());
                }
                break;
            }
            // A full Unix listener backlog reports EAGAIN, not a pending connection.
            Some(libc::EAGAIN) | Some(libc::EINTR) => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err(format!("The hardware helper is unavailable: {error}")),
        }
    }
    let frame = format!("{request}\n");
    let mut remaining = frame.as_bytes();
    while !remaining.is_empty() {
        control.check_io(id, deadline)?;
        match stream.write(remaining) {
            Ok(0) => return Err("The hardware helper closed the connection.".into()),
            Ok(count) => remaining = &remaining[count..],
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                ready(fd, libc::POLLOUT, control, id, deadline)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            // An ungranted peer can send its authoritative denial before receiving
            // any bytes. Consume that bounded reply even if its write side closed.
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    let mut response = Vec::new();
    let mut buffer = [0; 2048];
    loop {
        control.check_io(id, deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Err("Incomplete hardware helper response.".into()),
            Ok(count) => {
                response.extend_from_slice(&buffer[..count]);
                if response.len() > limit {
                    return Err("Hardware helper response exceeds its limit.".into());
                }
                if response.contains(&b'\n') {
                    return serde_json::from_slice(&response).map_err(|e| e.to_string());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                ready(stream.as_raw_fd(), libc::POLLIN, control, id, deadline)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn call_json<T: serde::de::DeserializeOwned>(
    _: &Path,
    _: &str,
    _: usize,
    _: &Control,
    _: Option<u64>,
    _: Instant,
) -> Result<T, String> {
    Err("Extended hardware monitoring is available on Linux.".into())
}

/// Reject an unsafe existing candidate rather than silently falling back to embedded root code.
#[cfg(target_os = "linux")]
fn installed_helper() -> Result<Option<PathBuf>, String> {
    use std::os::unix::fs::MetadataExt;
    for path in [
        "/usr/libexec/taskmanager-hardware",
        "/usr/local/libexec/taskmanager-hardware",
    ] {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        let path = Path::new(path);
        for component in path.ancestors().take_while(|p| !p.as_os_str().is_empty()) {
            let metadata = std::fs::symlink_metadata(component).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink()
                || metadata.uid() != 0
                || metadata.mode() & 0o022 != 0
                || (component == path && (!metadata.is_file() || metadata.mode() & 0o111 == 0))
                || (component != path && !metadata.is_dir())
            {
                return Err(
                    "The installed hardware helper path is not protected from unprivileged writes."
                        .into(),
                );
            }
        }
        return Ok(Some(path.to_owned()));
    }
    Ok(None)
}

#[cfg(not(target_os = "linux"))]
fn installed_helper() -> Result<Option<PathBuf>, String> {
    Err("Extended hardware monitoring is available on Linux.".into())
}

pub(crate) struct Access {
    pub permanent: PathBuf,
    child: Option<Child>,
    socket: Option<PathBuf>,
    retiring: Option<Child>,
}

impl Default for Access {
    fn default() -> Self {
        Self {
            permanent: "/run/taskmanager-hardware.sock".into(),
            child: None,
            socket: None,
            retiring: None,
        }
    }
}

impl Drop for Access {
    fn drop(&mut self) {
        // No detached wait thread per child. The worker normally reaps cooperatively.
        for child in [&mut self.child, &mut self.retiring].into_iter().flatten() {
            cancel_child(child);
            let _ = child.try_wait();
        }
    }
}

pub(crate) fn cancel_child(child: &mut Child) {
    child.stdin.take();
    // Cancels a still-unprivileged authorization process where permitted. Once elevated,
    // the helper itself observes EOF/pidfd; EPERM is not evidence of quiescence.
    let _ = child.kill();
}

impl Access {
    pub fn call(&self, request: &str, control: &Control, id: Option<u64>) -> Result<Reply, String> {
        call_json(
            self.socket.as_deref().unwrap_or(&self.permanent),
            request,
            8192,
            control,
            id,
            Instant::now() + IO_TIMEOUT,
        )
    }
    pub fn permanent_reply(
        &self,
        request: &str,
        control: &Control,
        id: u64,
    ) -> Result<Reply, String> {
        call_json(
            &self.permanent,
            request,
            8192,
            control,
            Some(id),
            Instant::now() + IO_TIMEOUT,
        )
    }
    pub fn read(&self, control: &Control) -> Result<Reply, String> {
        self.call("read", control, None)
    }
    pub fn read_spd(&self, control: &Control, id: u64) -> Result<Vec<crate::SpdEeprom>, String> {
        #[derive(Deserialize)]
        struct SpdReply {
            enabled: bool,
            spd: Option<Vec<crate::SpdEeprom>>,
            error: Option<String>,
        }
        let reply: SpdReply = call_json(
            self.socket.as_deref().unwrap_or(&self.permanent),
            "spd",
            131072,
            control,
            Some(id),
            Instant::now() + IO_TIMEOUT,
        )?;
        if reply.enabled {
            Ok(reply.spd.unwrap_or_default())
        } else {
            Err(reply
                .error
                .unwrap_or_else(|| "Hardware access is not authorized.".into()))
        }
    }
    pub fn stop_session(&mut self) {
        self.socket = None;
        if let Some(mut child) = self.child.take() {
            // Foreground admission waits until the sole retiring child has been reaped.
            debug_assert!(self.retiring.is_none());
            cancel_child(&mut child);
            self.retiring = Some(child);
        }
    }
    pub fn reap(&mut self) -> bool {
        if self
            .retiring
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_some())
        {
            self.retiring = None;
        }
        self.retiring.is_none()
    }
    pub fn adopt_session(&mut self, child: Child, socket: PathBuf) {
        debug_assert!(self.child.is_none());
        self.child = Some(child);
        self.socket = Some(socket);
    }
    #[cfg(test)]
    pub fn session_owned(&self) -> bool {
        self.child
            .as_ref()
            .is_some_and(|child| child.stdin.is_some())
            && self.socket.is_some()
    }
    /// None socket means installed enable/bootstrap; Some socket means Temporary startup.
    pub fn authorize(&self, install: bool) -> Result<(Child, Option<PathBuf>), String> {
        let helper = installed_helper()?;
        let (mut command, socket) = authorization_command(helper.as_deref(), install);
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok((child, socket))
    }
}

fn authorization_command(helper: Option<&Path>, install: bool) -> (Command, Option<PathBuf>) {
    let pid = std::process::id().to_string();
    let mut command = Command::new("/usr/bin/pkexec");
    let socket = if let Some(helper) = helper {
        command.arg(helper).args(["enable", &pid]);
        None
    } else {
        command.args(["/usr/bin/python3", "-I", "-c", &helper_code()]);
        if install {
            command.args(["install", &pid]);
            None
        } else {
            let token = format!("{:032x}", rand::random::<u128>());
            command.args(["session", &pid, &token]);
            Some(PathBuf::from(format!(
                "/run/taskmanager-hardware-{pid}-{token}.sock"
            )))
        }
    };
    (command, socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_route_never_contains_embedded_code_or_install_operation() {
        for path in [
            "/usr/libexec/taskmanager-hardware",
            "/usr/local/libexec/taskmanager-hardware",
        ] {
            for install in [false, true] {
                let (command, socket) = authorization_command(Some(Path::new(path)), install);
                let args: Vec<_> = command
                    .get_args()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                assert_eq!(command.get_program(), "/usr/bin/pkexec");
                assert_eq!(
                    args,
                    [
                        path.to_owned(),
                        "enable".into(),
                        std::process::id().to_string()
                    ]
                );
                assert!(socket.is_none());
            }
        }
        let (source, socket) = authorization_command(None, false);
        assert_eq!(source.get_args().next().unwrap(), "/usr/bin/python3");
        assert!(socket.is_some());
        let (bootstrap, socket) = authorization_command(None, true);
        assert_eq!(bootstrap.get_args().nth(4).unwrap(), "install");
        assert!(socket.is_none());
    }
    #[test]
    fn embedded_helper_and_install_payloads_are_valid_python() {
        let code = helper_code();
        let status = Command::new("/usr/bin/python3")
            .args(["-I", "-c", "import ast,sys; ast.parse(sys.argv[1])", &code])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(code.contains("INSTALL_SOURCE ="));
        assert!(code.contains("SERVICE_TEXT ="));
    }
    #[test]
    fn closing_session_closes_lifetime_and_reaps_child() {
        let mut access = Access::default();
        access.child = Some(
            Command::new("/usr/bin/python3")
                .args(["-I", "-c", "import sys; sys.stdin.buffer.read()"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        access.stop_session();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !access.reap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(access.child.is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn absolute_deadline_bounds_trickled_response_and_cancellation() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!(
            "tm-helper-test-{}-{}.sock",
            std::process::id(),
            rand::random::<u64>()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 32];
            assert!(stream.read(&mut request).unwrap() > 0);
            for _ in 0..20 {
                if stream.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        });
        let control = Control::new(Arc::new(AtomicBool::new(true)));
        let now = Instant::now();
        let response: Result<Reply, _> = call_json(
            &path,
            "read",
            8192,
            &control,
            Some(1),
            now + Duration::from_millis(80),
        );
        assert!(response.is_err());
        assert!(now.elapsed() < Duration::from_millis(250));
        control.cancelled.store(1, Ordering::Release);
        let response: Result<Reply, _> = call_json(
            &path,
            "read",
            8192,
            &control,
            Some(1),
            Instant::now() + IO_TIMEOUT,
        );
        assert!(response.unwrap_err().contains("cancelled"));
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
