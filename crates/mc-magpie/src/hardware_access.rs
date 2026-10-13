/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! Optional host telemetry. All calls run on the gatherer thread, with bounded reads/timeouts.
use serde::{Deserialize, Serialize};

const HELPER: &str = include_str!("../../../support/taskmanager-hardware");

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
        // JSON string literals are valid Python strings for these ASCII source payloads.
        code.push_str(&format!(
            "{name} = {}\n",
            serde_json::to_string(text).unwrap()
        ));
    }
    code.push_str(HELPER);
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_helper_and_install_payloads_are_valid_python() {
        let code = helper_code();
        let status = std::process::Command::new("python3")
            .args(["-I", "-c", "import ast,sys; ast.parse(sys.argv[1])", &code])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(code.contains("INSTALL_SOURCE ="));
        assert!(code.contains("SERVICE_TEXT ="));
    }

    #[test]
    fn dropping_session_closes_child_lifetime_pipe() {
        use std::io::Read;
        let mut child = std::process::Command::new("python3")
            .args([
                "-I",
                "-c",
                "import sys; sys.stdin.buffer.read(); print('closed')",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap();
        let access = Access {
            child: Some(child),
            socket: None,
            running: None,
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut text = String::new();
            output.read_to_string(&mut text).unwrap();
            sender.send(text).unwrap();
        });
        drop(access);
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .trim(),
            "closed"
        );
    }
}

#[derive(Default)]
pub(crate) struct Access {
    child: Option<std::process::Child>,
    socket: Option<std::path::PathBuf>,
    running: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Drop for Access {
    fn drop(&mut self) {
        // Dropping stdin stops the root helper even when the GUI crashes or closes while
        // authorization is pending. No blocking IPC is needed on the rendering thread.
        if let Some(mut child) = self.child.take() {
            child.stdin.take();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
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

#[derive(Deserialize)]
struct Reply {
    enabled: bool,
    data: Option<HardwareTelemetry>,
    error: Option<String>,
}

#[cfg(target_os = "linux")]
fn call_json<T: serde::de::DeserializeOwned>(
    socket: &std::path::Path,
    request: &str,
    limit: u64,
) -> Result<T, String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let mut stream = UnixStream::connect(socket)
        .map_err(|_| "The hardware helper is unavailable.".to_owned())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    stream
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn call_json<T: serde::de::DeserializeOwned>(
    _: &std::path::Path,
    _: &str,
    _: u64,
) -> Result<T, String> {
    Err("Extended hardware monitoring is available on Linux.".into())
}

impl Access {
    pub(crate) fn new(running: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self {
            running: Some(running),
            child: None,
            socket: None,
        }
    }
    fn reset(&mut self) {
        let running = self.running.clone();
        *self = Self {
            running,
            child: None,
            socket: None,
        };
    }
    fn path(&self) -> &std::path::Path {
        self.socket
            .as_deref()
            .unwrap_or(std::path::Path::new("/run/taskmanager-hardware.sock"))
    }

    fn call(&self, request: &str) -> Result<Reply, String> {
        call_json(self.path(), request, 8192)
    }

    pub(crate) fn read_spd(&self) -> Result<Vec<crate::SpdEeprom>, String> {
        #[derive(Deserialize)]
        struct SpdReply {
            enabled: bool,
            spd: Option<Vec<crate::SpdEeprom>>,
        }
        let reply: SpdReply = call_json(self.path(), "spd", 131072)?;
        if reply.enabled {
            Ok(reply.spd.unwrap_or_default())
        } else {
            Err("Hardware access is not authorized.".into())
        }
    }

    pub(crate) fn read(&self) -> Result<HardwareTelemetry, String> {
        let reply = self.call("read")?;
        if reply.enabled {
            Ok(reply.data.unwrap_or_default())
        } else {
            Err(reply
                .error
                .unwrap_or_else(|| "Hardware access is not authorized.".into()))
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, interactive: bool) -> Result<bool, String> {
        if !enabled {
            let result = self.call("disable").map(|reply| reply.enabled);
            if self.child.is_some() {
                self.reset();
                return Ok(false);
            }
            return result;
        }
        // A previously authorized permanent service can be reused without authentication.
        if self.call("read").is_ok_and(|reply| reply.enabled) {
            return Ok(true);
        }
        if !interactive {
            return Err("Enable hardware access in Options to authorize this account.".into());
        }
        self.start_session()
    }

    fn python() -> std::process::Command {
        let mut command = std::process::Command::new("/usr/bin/pkexec");
        command.args(["/usr/bin/python3", "-I", "-c", &helper_code()]);
        command
    }

    fn start_session(&mut self) -> Result<bool, String> {
        #[cfg(not(target_os = "linux"))]
        return Err("Extended hardware monitoring is available on Linux.".into());
        #[cfg(target_os = "linux")]
        {
            use std::time::{Duration, Instant};
            let token = format!("{:032x}", rand::random::<u128>());
            let pid = std::process::id().to_string();
            let child = Self::python()
                .args(["session", &pid, &token])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            self.child = Some(child);
            self.socket = Some(format!("/run/taskmanager-hardware-{pid}-{token}.sock").into());
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                if self.read().is_ok() {
                    return Ok(true);
                }
                if self
                    .child
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_some()
                {
                    self.reset();
                    return Err(
                        "Hardware authorization was cancelled or the temporary helper failed."
                            .into(),
                    );
                }
                if Instant::now() >= deadline
                    || self
                        .running
                        .as_ref()
                        .is_some_and(|running| !running.load(std::sync::atomic::Ordering::Acquire))
                {
                    self.reset();
                    return Err("Hardware authorization timed out.".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    pub(crate) fn install(&mut self) -> Result<bool, String> {
        use std::time::{Duration, Instant};
        let mut installer = Self::new(self.running.clone().unwrap_or_default());
        installer.child = Some(
            Self::python()
                .args(["install", &std::process::id().to_string()])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(status) = installer
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .map_err(|e| e.to_string())?
            {
                if !status.success() {
                    return Err("Service installation was cancelled or failed.".into());
                }
                break;
            }
            if Instant::now() >= deadline
                || self
                    .running
                    .as_ref()
                    .is_some_and(|running| !running.load(std::sync::atomic::Ordering::Acquire))
            {
                return Err(
                    "Service installation authorization timed out or Task Manager closed.".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.reset();
        self.read().map(|_| true)
    }
}
