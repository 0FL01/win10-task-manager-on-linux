// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Task Manager contributors
// Local implementation of Bolt/HID++ wire formats, checked against Solaar 1.1.20.
// No Solaar lifecycle/configuration code is used. See docs/PROVENANCE.md.

use super::Battery;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(30);
const MAX_AGE: Duration = Duration::from_secs(90);
const SOFTWARE_ID: u8 = 0x06; // Kernel: 1; Solaar 1.1.20: B; notifications: 0.

struct Snapshot {
    observed: Instant,
    batteries: Vec<Battery>,
}

#[derive(Default)]
struct State {
    stop: bool,
    latest: Option<Snapshot>,
}

type Shared = (Mutex<State>, Condvar);

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    // A failed fallback worker must never poison the generic battery refresh.
    shared.0.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) struct Collector {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Collector {
    pub(super) fn new() -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let state = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("hidpp-battery".into())
            .spawn(move || loop {
                let batteries = collect(&state);
                let mut guard = lock(&state);
                if guard.stop {
                    break;
                }
                guard.latest = Some(Snapshot {
                    observed: Instant::now(),
                    batteries,
                });
                let (guard, _) = state
                    .1
                    .wait_timeout_while(guard, INTERVAL, |s| !s.stop)
                    .unwrap_or_else(|e| e.into_inner());
                if guard.stop {
                    break;
                }
            })
            .map_err(|e| log::warn!("Could not start mouse battery collector: {e}"))
            .ok();
        Self { shared, worker }
    }

    pub(super) fn merge(&self, generic: &[Battery], output: &mut Vec<Battery>) {
        merge(
            generic,
            lock(&self.shared).latest.as_ref(),
            Instant::now(),
            output,
        );
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        lock(&self.shared).stop = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serial(value: &str) -> Option<String> {
    let value: String = value.trim().chars().filter(|c| *c != '-').collect();
    (value.len() == 8
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value != "00000000"
        && !value.eq_ignore_ascii_case("FFFFFFFF"))
    .then(|| value.to_ascii_uppercase())
}

fn same_device(generic: &Battery, fallback: &Battery) -> bool {
    generic.kind == fallback.kind
        && generic
            .vendor
            .as_deref()
            .zip(fallback.vendor.as_deref())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        && generic
            .serial
            .as_deref()
            .and_then(serial)
            .zip(fallback.serial.as_deref().and_then(serial))
            .is_some_and(|(a, b)| a == b)
}

fn merge(generic: &[Battery], latest: Option<&Snapshot>, now: Instant, output: &mut Vec<Battery>) {
    output.clear();
    output.extend_from_slice(generic);
    if let Some(snapshot) = latest.filter(|s| now.duration_since(s.observed) <= MAX_AGE) {
        output.extend(
            snapshot
                .batteries
                .iter()
                .filter(|b| !generic.iter().any(|g| same_device(g, b)))
                .cloned(),
        );
    }
}

fn receivers() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let Ok(device) = fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        let Ok(uevent) = fs::read_to_string(device.join("uevent")) else {
            continue;
        };
        if !uevent.lines().any(|s| s == "HID_ID=0003:0000046D:0000C548") {
            continue;
        }
        let interface = device
            .ancestors()
            .find_map(|p| fs::read_to_string(p.join("bInterfaceNumber")).ok());
        if interface.is_some_and(|s| s.trim() == "02") {
            paths.push(PathBuf::from("/dev").join(entry.file_name()));
        }
    }
    paths.sort();
    paths
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn collect(shared: &Shared) -> Vec<Battery> {
    let mut batteries = Vec::new();
    for path in receivers() {
        if lock(shared).stop {
            break;
        }
        // Fresh per-cycle queues; never leave an unread hidraw fd open for 30 s.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path);
        let Ok(file) = file else {
            log::debug!("Mouse battery endpoint unavailable: {}", path.display());
            continue;
        };
        let mut connection = Connection { file, shared };
        let Ok(uid) = connection.request(0xFF, 0x83, 0xFB, &[]) else {
            continue;
        };
        if uid.len() != 16 {
            continue;
        }
        for slot in 1..=6 {
            if lock(shared).stop {
                break;
            }
            if let Ok(Some(battery)) = connection.mouse(&uid, slot) {
                batteries.push(battery);
            }
        }
    }
    batteries
}

struct Connection<'a> {
    file: File,
    shared: &'a Shared,
}

impl Connection<'_> {
    fn ready(&self, events: i16, deadline: Instant) -> io::Result<()> {
        loop {
            if lock(self.shared).stop {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let mut descriptor = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events,
                revents: 0,
            };
            let timeout = remaining.as_millis().clamp(1, 50) as i32;
            // SAFETY: descriptor is valid for one pollfd and File owns its fd.
            let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::ErrorKind::NotConnected.into());
            }
            if descriptor.revents & events != 0 {
                return Ok(());
            }
        }
    }

    fn request(
        &mut self,
        slot: u8,
        command: u8,
        function: u8,
        params: &[u8],
    ) -> io::Result<Vec<u8>> {
        let mut request = [0; 20];
        let length = if slot == 0xFF { 7 } else { 20 };
        request[..4].copy_from_slice(&[
            if slot == 0xFF { 0x10 } else { 0x11 },
            slot,
            command,
            function,
        ]);
        request[4..4 + params.len()].copy_from_slice(params);
        self.exchange(
            &request[..length],
            Duration::from_millis(if slot == 0xFF { 900 } else { 4000 }),
        )
    }

    fn exchange(&mut self, request: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let mut report = [0; 64];
        // Bounded drain BEFORE writing, so an old/full queue cannot lose our reply.
        loop {
            if lock(self.shared).stop || Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match self.file.read(&mut report) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        loop {
            if lock(self.shared).stop || Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            // hidraw poll reports input readiness, not POLLOUT. Attempt the
            // atomic report write first; slow kernel writes stay on this worker.
            match self.file.write(request) {
                Ok(n) if n == request.len() => break,
                Ok(_) => return Err(io::ErrorKind::WriteZero.into()),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.ready(libc::POLLOUT, deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e),
            }
        }
        loop {
            self.ready(libc::POLLIN, deadline)?;
            match self.file.read(&mut report) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    if let Some(result) = matching_reply(&report[..n], request) {
                        return result.map(<[u8]>::to_vec);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    ()
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn feature(&mut self, slot: u8, id: u16) -> io::Result<Option<u8>> {
        let reply = self.request(slot, 0, SOFTWARE_ID, &id.to_be_bytes())?;
        Ok(reply.first().copied().filter(|index| *index != 0))
    }

    fn mouse(&mut self, uid: &[u8], slot: u8) -> io::Result<Option<Battery>> {
        let pair = self.request(0xFF, 0x83, 0xB5, &[0x50 + slot])?;
        if pair.len() < 8 || pair[1] & 0x0F != 2 {
            return Ok(None);
        }
        let Some(index) = self.feature(slot, 0x1004)? else {
            return Ok(None);
        };
        let capabilities = self.request(slot, index, SOFTWARE_ID, &[])?;
        if !capabilities.get(1).is_some_and(|b| b & 0x02 != 0) {
            return Ok(None);
        }
        let name = self.request(0xFF, 0x83, 0xB5, &[0x60 + slot, 1])?;
        let Some(length) = name.get(2) else {
            return Ok(None);
        };
        let end = (3 + usize::from(*length).min(14)).min(name.len());
        let model = String::from_utf8_lossy(&name[3..end])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if model.is_empty() {
            return Ok(None);
        }
        // Pairing serial and Unit ID are different namespaces. Export Unit ID for
        // exact comparison with the kernel/UPower serial; pairing identifies our key.
        let unit_id = self.feature(slot, 0x0003).ok().flatten().and_then(|index| {
            self.request(slot, index, SOFTWARE_ID, &[])
                .ok()
                .and_then(|info| info.get(1..5).map(hex))
                .and_then(|id| serial(&id))
        });
        let status = self.request(slot, index, 0x10 | SOFTWARE_ID, &[])?;
        let Some((percentage, state)) = decode_battery(&capabilities, &status) else {
            return Ok(None);
        };
        Ok(Some(Battery {
            name: format!("hidpp-bolt-{}-{}", hex(uid), hex(&pair[4..8])),
            vendor: Some("Logitech".into()),
            model: Some(model),
            serial: unit_id,
            kind: Some(5),
            power_supply: Some(false),
            percentage,
            state,
            ..Battery::default()
        }))
    }
}

fn matching_reply<'a>(report: &'a [u8], request: &[u8]) -> Option<io::Result<&'a [u8]>> {
    if !matches!(
        (report.first(), report.len()),
        (Some(0x10), 7) | (Some(0x11), 20)
    ) || report[1] != request[1]
    {
        return None;
    }
    if report[2..4] == request[2..4] {
        if request[1..4] == [0xFF, 0x83, 0xB5] && report[4] != request[4] {
            return None;
        }
        return Some(Ok(&report[4..]));
    }
    if matches!(report[2], 0x8F | 0xFF) && report[3..5] == request[2..4] {
        return Some(Err(io::Error::other("HID++ getter rejected")));
    }
    None
}

fn decode_battery(capabilities: &[u8], status: &[u8]) -> Option<(f32, Option<i32>)> {
    if capabilities.get(1)? & 0x02 == 0 || status.len() < 4 || status[0] > 100 {
        return None;
    }
    let state = match status[2] {
        0 => Some(2),
        1 | 2 => Some(1),
        3 => Some(4),
        _ => None,
    };
    Some((f32::from(status[0]) / 100., state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};

    #[test]
    fn unified_battery_preserves_zero_and_uses_its_own_status_enum() {
        assert_eq!(decode_battery(&[15, 3], &[0, 8, 0, 0]), Some((0., Some(2))));
        for (wire, expected) in [
            (1, Some(1)),
            (2, Some(1)),
            (3, Some(4)),
            (4, None),
            (255, None),
        ] {
            assert_eq!(
                decode_battery(&[15, 2], &[100, 8, wire, 1]),
                Some((1., expected))
            );
        }
        assert_eq!(decode_battery(&[15, 1], &[0, 8, 0, 0]), None);
        assert_eq!(decode_battery(&[15, 2], &[101, 8, 0, 0]), None);
        assert_eq!(decode_battery(&[15], &[50, 4, 0, 0]), None);
        assert_eq!(decode_battery(&[15, 2], &[50]), None);
    }

    #[test]
    fn replies_match_slot_function_software_id_and_receiver_selector() {
        let mut request = [0; 20];
        request[..4].copy_from_slice(&[0x11, 2, 8, 0x16]);
        let mut report = request;
        report[4] = 50;
        assert_eq!(matching_reply(&report, &request).unwrap().unwrap()[0], 50);
        for (field, value) in [(1, 1), (2, 9), (3, 0x10), (3, 0x11), (3, 0x1B), (3, 0x26)] {
            let mut unrelated = report;
            unrelated[field] = value;
            assert!(matching_reply(&unrelated, &request).is_none());
        }
        assert!(matching_reply(&report[..6], &request).is_none());
        let mut error = [0; 20];
        error[..6].copy_from_slice(&[0x11, 2, 0xFF, 8, 0x16, 2]);
        assert!(matching_reply(&error, &request).unwrap().is_err());
        let rap = [0x10, 0xFF, 0x83, 0xB5, 0x56, 0, 0];
        report[..5].copy_from_slice(&[0x11, 0xFF, 0x83, 0xB5, 0x51]);
        assert!(matching_reply(&report, &rap).is_none());
        report[4] = 0x56;
        assert!(matching_reply(&report, &rap).unwrap().is_ok());
        assert!(matching_reply(&[0x10, 0xFF, 0x8F, 0x83, 0xB5, 2, 0], &rap)
            .unwrap()
            .is_err());
    }

    #[test]
    fn latest_snapshot_replaces_devices_and_deduplicates_only_exact_identity() {
        let now = Instant::now();
        let mouse = Battery {
            name: "hidpp-bolt-RECEIVER-PAIR-A".into(),
            kind: Some(5),
            vendor: Some("Logitech".into()),
            model: Some("MX Master 3S".into()),
            serial: Some("1234ABCD".into()),
            power_supply: Some(false),
            ..Battery::default()
        };
        let mut generic = mouse.clone();
        generic.name = "kernel-path".into();
        generic.serial = Some("12-34-ab-cd".into());
        let mut snapshot = Snapshot {
            observed: now,
            batteries: vec![mouse.clone()],
        };
        let mut output = Vec::new();
        merge(&[], None, now, &mut output);
        assert!(output.is_empty());
        merge(&[generic.clone()], Some(&snapshot), now, &mut output);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].name, "kernel-path");
        generic.serial = Some("87654321".into());
        merge(&[generic], Some(&snapshot), now, &mut output);
        assert_eq!(output.len(), 2); // Same model is not the same device.
        merge(
            &[],
            Some(&snapshot),
            now + MAX_AGE + Duration::from_secs(1),
            &mut output,
        );
        assert!(output.is_empty());
        assert_eq!(snapshot.observed, now); // Reading never refreshes the age.
        snapshot.batteries.clear();
        merge(&[], Some(&snapshot), now, &mut output);
        assert!(output.is_empty());
        let mut replacement = mouse;
        replacement.name = "hidpp-bolt-RECEIVER-PAIR-B".into();
        snapshot.batteries.push(replacement);
        merge(&[], Some(&snapshot), now, &mut output);
        assert_eq!(output.len(), 1);
        assert!(output[0].name.ends_with("PAIR-B"));
    }

    #[test]
    fn foreign_reports_do_not_extend_the_transaction_deadline() {
        let mut fds = [0; 2];
        // SAFETY: fds has room for both descriptors; ownership is transferred below.
        assert_eq!(
            unsafe {
                libc::socketpair(
                    libc::AF_UNIX,
                    libc::SOCK_SEQPACKET | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                    0,
                    fds.as_mut_ptr(),
                )
            },
            0
        );
        // SAFETY: socketpair returned two distinct, owned descriptors.
        let file = File::from(unsafe { OwnedFd::from_raw_fd(fds[0]) });
        let mut peer = File::from(unsafe { OwnedFd::from_raw_fd(fds[1]) });
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let stop = Arc::clone(&shared);
        let sender = thread::spawn(move || {
            let mut request = [0; 64];
            let mut started = false;
            while !lock(&stop).stop {
                started |= peer.read(&mut request).is_ok();
                if started {
                    let _ = peer.write(&[
                        0x11, 2, 8, 0x1B, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ]);
                }
                thread::yield_now();
            }
        });
        let mut connection = Connection {
            file,
            shared: &shared,
        };
        let start = Instant::now();
        let result = connection.exchange(
            &[
                0x11, 2, 8, 0x16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            Duration::from_millis(50),
        );
        lock(&shared).stop = true;
        sender.join().unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[ignore = "requires a user-accessible Bolt receiver and an awake supported mouse"]
    fn host_bolt_battery_getters() {
        let shared = (Mutex::new(State::default()), Condvar::new());
        let batteries = collect(&shared);
        assert!(!batteries.is_empty(), "no supported awake mouse accessible");
        for b in batteries {
            assert_eq!(b.kind, Some(5));
            assert_eq!(b.power_supply, Some(false));
            assert!((0. ..=1.).contains(&b.percentage));
            assert!(b.energy.is_none() && b.power.is_none() && b.history.is_empty());
            println!(
                "{}: {:.0}%, state={:?}",
                b.model.as_deref().unwrap(),
                b.percentage * 100.,
                b.state
            );
        }
    }
}
