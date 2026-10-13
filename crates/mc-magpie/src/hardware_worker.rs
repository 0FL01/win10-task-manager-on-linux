/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! One hardware-specific worker: one foreground operation, one latest sample, no daemon IPC.
use crate::hardware_access::{call_json, cancel_child, Access, Control, Reply, IO_TIMEOUT};
use crate::{
    Event, HardwareGrant, HardwareMode, HardwareOperation, HardwareStatus, HardwareTelemetry,
    LatestSlot, Waker, INTERVAL_STEP,
};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(20);
const AUTH_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct Sample {
    pub epoch: u64,
    pub at: Instant,
    pub data: Option<HardwareTelemetry>,
}

pub(crate) struct Handle {
    requests: SyncSender<(u64, HardwareOperation)>,
    pub control: Arc<Control>,
    pub samples: Arc<LatestSlot<Sample>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    pub fn spawn(
        available: bool,
        running: Arc<AtomicBool>,
        speed: Arc<AtomicU64>,
        events: SyncSender<Event>,
        waker: Waker,
    ) -> Self {
        let control = Arc::new(Control::new(running));
        let samples = Arc::new(LatestSlot::new());
        let worker = Worker {
            access: Access::default(),
            available,
            control: control.clone(),
            samples: samples.clone(),
            status: HardwareStatus::default(),
            pending: None,
            notification: None,
            next_sample: Instant::now(),
        };
        Self::start(worker, speed, events, waker)
    }
    fn start(
        mut worker: Worker,
        speed: Arc<AtomicU64>,
        events: SyncSender<Event>,
        waker: Waker,
    ) -> Self {
        let control = worker.control.clone();
        let samples = worker.samples.clone();
        let (requests, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("mc-hardware".into())
            .spawn(move || worker.run(receiver, speed, events, waker))
            .expect("Failed to start hardware worker");
        Self {
            requests,
            control,
            samples,
            thread: Some(thread),
        }
    }
    #[cfg(test)]
    pub(crate) fn pending_authorization_for_test(
        running: Arc<AtomicBool>,
        speed: Arc<AtomicU64>,
        events: SyncSender<Event>,
        waker: Waker,
    ) -> Self {
        let control = Arc::new(Control::new(running));
        control.active.store(1, Ordering::Release);
        let mut access = Access::default();
        access.permanent =
            std::env::temp_dir().join(format!("tm-absent-{}", rand::random::<u64>()));
        let child = std::process::Command::new("/usr/bin/python3")
            .args(["-I", "-c", "import sys; sys.stdin.buffer.read()"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let worker = Worker {
            access,
            available: true,
            control,
            samples: Arc::new(LatestSlot::new()),
            status: HardwareStatus::default(),
            pending: Some(Pending {
                id: 1,
                operation: HardwareOperation::Install,
                deadline: Instant::now() + AUTH_TIMEOUT,
                child: Some(child),
                socket: None,
                cancelling: false,
                completion: None,
            }),
            notification: None,
            next_sample: Instant::now(),
        };
        Self::start(worker, speed, events, waker)
    }
    pub fn submit(&self, id: u64, operation: HardwareOperation) -> bool {
        if id == 0
            || !self.control.running.load(Ordering::Acquire)
            || self
                .control
                .active
                .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        self.control.cancelled.store(0, Ordering::Release);
        if operation == HardwareOperation::Disable {
            self.control.sampling.store(false, Ordering::Release);
            self.control.invalidate();
        }
        if self.requests.try_send((id, operation)).is_err() {
            self.control.active.store(0, Ordering::Release);
            return false;
        }
        true
    }
    pub fn cancel(&self, id: u64) -> bool {
        if id == 0 || self.control.active.load(Ordering::Acquire) != id {
            return false;
        }
        self.control.cancelled.store(id, Ordering::Release);
        true
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.control.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let deadline = Instant::now() + IO_TIMEOUT + TICK;
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(TICK);
            }
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                // A kernel EEPROM read can be uninterruptible. Never turn it into an
                // unbounded GUI join; this is one worker, not one detached thread per read.
                log::warn!("Hardware worker is still returning from host kernel I/O.");
            }
        }
    }
}

struct Pending {
    id: u64,
    operation: HardwareOperation,
    deadline: Instant,
    child: Option<Child>,
    socket: Option<PathBuf>,
    cancelling: bool,
    completion: Option<Event>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            cancel_child(child);
            let _ = child.try_wait();
        }
    }
}

struct Worker {
    access: Access,
    available: bool,
    control: Arc<Control>,
    samples: Arc<LatestSlot<Sample>>,
    status: HardwareStatus,
    pending: Option<Pending>,
    notification: Option<Event>,
    next_sample: Instant,
}

impl Worker {
    fn invalidate(&mut self) {
        self.status.epoch = self.control.invalidate();
        self.samples.publish(Sample {
            epoch: self.status.epoch,
            at: Instant::now(),
            data: None,
        });
    }
    fn enable(&mut self, mode: HardwareMode, data: HardwareTelemetry) {
        self.invalidate();
        self.status.mode = mode;
        self.status.sampling = true;
        self.control.sampling.store(true, Ordering::Release);
        self.samples.publish(Sample {
            epoch: self.status.epoch,
            at: Instant::now(),
            data: Some(data),
        });
    }
    fn completion(&self, id: u64, result: Result<(), String>) -> Event {
        Event::HardwareAccess {
            id: Some(id),
            status: self.status,
            result,
        }
    }
    fn observe_grant(&mut self, reply: &Reply) {
        self.status.grant = if reply.enabled {
            HardwareGrant::Present
        } else if reply.revoked {
            HardwareGrant::Absent
        } else {
            HardwareGrant::Unconfirmed
        };
    }
    fn begin(&mut self, id: u64, operation: HardwareOperation) -> Pending {
        self.status.epoch = self.control.epoch.load(Ordering::Acquire);
        let mut pending = Pending {
            id,
            operation,
            deadline: Instant::now() + AUTH_TIMEOUT,
            child: None,
            socket: None,
            cancelling: false,
            completion: None,
        };
        if !self.available {
            pending.completion = Some(if operation == HardwareOperation::Spd {
                Event::Spd {
                    id,
                    epoch: self.status.epoch,
                    privileged: false,
                    result: Err("SPD is unavailable for recorded or synthetic data.".into()),
                }
            } else {
                self.completion(
                    id,
                    Err("Hardware access is unavailable for recorded or synthetic data.".into()),
                )
            });
            return pending;
        }
        match operation {
            HardwareOperation::Spd => {
                let epoch = self.status.epoch;
                let privileged = self.status.sampling;
                let deadline = Instant::now() + IO_TIMEOUT;
                let result = if privileged {
                    self.access.read_spd(&self.control, id)
                } else {
                    crate::spd::read(|| self.control.check(id, deadline))
                };
                pending.completion = Some(Event::Spd {
                    id,
                    epoch,
                    privileged,
                    result,
                });
            }
            HardwareOperation::Disable => {
                self.status.epoch = self.control.epoch.load(Ordering::Acquire);
                self.status.sampling = false;
                self.status.mode = HardwareMode::Off;
                self.access.stop_session();
                self.samples.publish(Sample {
                    epoch: self.status.epoch,
                    at: Instant::now(),
                    data: None,
                });
                pending.deadline = Instant::now() + IO_TIMEOUT * 2;
            }
            HardwareOperation::Enable { interactive } => {
                if self.status.mode == HardwareMode::Temporary && self.status.sampling {
                    pending.completion = Some(self.completion(id, Ok(())));
                    return pending;
                }
                let probe = self.access.permanent_reply("read", &self.control, id);
                if let Ok(reply) = &probe {
                    self.observe_grant(reply);
                }
                if let Ok(reply) = probe {
                    if reply.enabled {
                        self.access.stop_session();
                        self.enable(HardwareMode::Persistent, reply.data.unwrap_or_default());
                        pending.completion = Some(self.completion(id, Ok(())));
                        return pending;
                    }
                }
                if !interactive {
                    pending.completion = Some(self.completion(
                        id,
                        Err("Enable hardware access in Options to authorize this account.".into()),
                    ));
                } else {
                    self.start_authorization(&mut pending, false);
                }
            }
            HardwareOperation::Install => self.start_authorization(&mut pending, true),
        }
        pending
    }
    fn start_authorization(&mut self, pending: &mut Pending, install: bool) {
        if self.control.check(pending.id, pending.deadline).is_err() {
            pending.completion =
                Some(self.completion(pending.id, Err("Hardware operation cancelled.".into())));
            return;
        }
        match self.access.authorize(install) {
            Ok((child, socket)) => {
                if socket.is_none() {
                    self.status.grant = HardwareGrant::Unconfirmed;
                }
                pending.child = Some(child);
                pending.socket = socket;
            }
            Err(error) => pending.completion = Some(self.completion(pending.id, Err(error))),
        }
    }
    fn poll(&mut self, pending: &mut Pending) {
        if pending.completion.is_some() {
            return;
        }
        if pending.child.is_none() {
            debug_assert_eq!(pending.operation, HardwareOperation::Disable);
            if let Err(error) = self.control.check(pending.id, pending.deadline) {
                self.status.grant = HardwareGrant::Unconfirmed;
                pending.completion = Some(self.completion(pending.id, Err(format!("Local monitoring stopped; permanent access revocation is unconfirmed: {error}"))));
                return;
            }
            match self
                .access
                .permanent_reply("disable", &self.control, pending.id)
            {
                Ok(reply) if reply.revoked => {
                    self.status.grant = HardwareGrant::Absent;
                    pending.completion = Some(self.completion(pending.id, Ok(())));
                }
                Ok(reply) if reply.busy => {}
                Ok(reply) => {
                    self.status.grant = HardwareGrant::Unconfirmed;
                    pending.completion = Some(self.completion(
                        pending.id,
                        Err(reply.error.unwrap_or_else(|| {
                            "Permanent access revocation is unconfirmed.".into()
                        })),
                    ));
                }
                Err(error) => {
                    self.status.grant = HardwareGrant::Unconfirmed;
                    pending.completion = Some(self.completion(pending.id, Err(format!("Local monitoring stopped; permanent access revocation is unconfirmed: {error}"))));
                }
            }
            return;
        }
        if !pending.cancelling && self.control.check(pending.id, pending.deadline).is_err() {
            pending.cancelling = true;
            cancel_child(pending.child.as_mut().unwrap());
            self.notification = Some(Event::HardwareAccess {
                id: None,
                status: self.status,
                result: Err(
                    "Cancellation requested; waiting for the authorized operation to finish."
                        .into(),
                ),
            });
        }
        match pending.child.as_mut().unwrap().try_wait() {
            Ok(Some(exit)) => {
                pending.child = None;
                if pending.cancelling {
                    // A timeout/EOF is not rollback. Probe only after the helper is quiescent.
                    if pending.socket.is_none() {
                        if let Ok(reply) = self.access.permanent_reply("read", &self.control, 0) {
                            self.observe_grant(&reply);
                        }
                    }
                    pending.completion = Some(self.completion(pending.id, Err("Hardware operation cancelled; existing temporary access was preserved. Check permanent access status before revoking it.".into())));
                } else if !exit.success() {
                    pending.completion = Some(self.completion(pending.id, Err("Hardware authorization was cancelled or the authorized helper failed.".into())));
                } else if pending.socket.is_some() {
                    pending.completion = Some(self.completion(
                        pending.id,
                        Err("The temporary helper exited before it became ready.".into()),
                    ));
                } else {
                    match self.access.permanent_reply("read", &self.control, pending.id) {
                        Ok(reply) if reply.enabled => {
                            self.observe_grant(&reply);
                            // Verify this explicit permanent endpoint BEFORE stopping Temporary.
                            self.access.stop_session();
                            self.enable(HardwareMode::Persistent, reply.data.unwrap_or_default());
                            pending.completion = Some(self.completion(pending.id, Ok(())));
                        }
                        Ok(reply) => {
                            self.observe_grant(&reply);
                            pending.completion = Some(self.completion(pending.id, Err("The permanent helper is not ready; temporary access was preserved.".into())));
                        }
                        Err(error) => pending.completion = Some(self.completion(pending.id, Err(format!("Permanent helper verification failed; temporary access was preserved: {error}")))),
                    }
                }
            }
            Ok(None) => {
                if !pending.cancelling {
                    if let Some(socket) = pending.socket.as_ref().filter(|p| p.exists()) {
                        let reply: Result<Reply, _> = call_json(
                            socket,
                            "read",
                            8192,
                            &self.control,
                            Some(pending.id),
                            Instant::now() + IO_TIMEOUT,
                        );
                        if let Ok(reply) = reply {
                            if reply.enabled {
                                let child = pending.child.take().unwrap();
                                self.access
                                    .adopt_session(child, pending.socket.take().unwrap());
                                self.enable(
                                    HardwareMode::Temporary,
                                    reply.data.unwrap_or_default(),
                                );
                                pending.completion = Some(self.completion(pending.id, Ok(())));
                            }
                        }
                    }
                }
            }
            Err(error) => {
                // Do not advertise quiescence or admit another grant-changing child.
                pending.cancelling = true;
                cancel_child(pending.child.as_mut().unwrap());
                self.notification = Some(Event::HardwareAccess {
                    id: None,
                    status: self.status,
                    result: Err(format!(
                        "Cannot confirm authorized helper termination: {error}"
                    )),
                });
            }
        }
    }
    fn sample(&mut self) {
        let reply = self.access.read(&self.control);
        self.apply_sample(reply);
    }
    fn apply_sample(&mut self, reply: Result<Reply, String>) {
        let error = match reply {
            Ok(reply) => {
                if self.status.mode == HardwareMode::Persistent {
                    self.observe_grant(&reply);
                }
                if reply.enabled {
                    self.samples.publish(Sample {
                        epoch: self.status.epoch,
                        at: Instant::now(),
                        data: Some(reply.data.unwrap_or_default()),
                    });
                    return;
                }
                reply
                    .error
                    .unwrap_or_else(|| "Hardware access is not authorized.".into())
            }
            // An interrupted transport proves nothing; a received denial stays authoritative.
            Err(_) if self.control.sampling_interrupted() => return,
            Err(error) => error,
        };
        if self.status.mode == HardwareMode::Temporary {
            self.access.stop_session();
            self.status.mode = HardwareMode::Off;
        }
        self.status.sampling = false;
        self.control.sampling.store(false, Ordering::Release);
        self.invalidate();
        self.notification = Some(Event::HardwareAccess {
            id: None,
            status: self.status,
            result: Err(error),
        });
    }
    fn run(
        &mut self,
        receiver: Receiver<(u64, HardwareOperation)>,
        speed: Arc<AtomicU64>,
        events: SyncSender<Event>,
        waker: Waker,
    ) {
        while self.control.running.load(Ordering::Acquire) {
            let reaped = self.access.reap();
            if self.pending.is_none() && reaped {
                if let Ok((id, operation)) = receiver.try_recv() {
                    self.pending = Some(self.begin(id, operation));
                }
            }
            if let Some(mut pending) = self.pending.take() {
                self.poll(&mut pending);
                if self.access.reap() {
                    if let Some(event) = pending.completion.take() {
                        match events.try_send(event) {
                            Ok(()) => {
                                self.control.active.store(0, Ordering::Release);
                                waker();
                            }
                            Err(TrySendError::Full(event)) => {
                                pending.completion = Some(event);
                                self.pending = Some(pending);
                                waker();
                            }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    } else {
                        self.pending = Some(pending);
                    }
                } else {
                    self.pending = Some(pending);
                }
            }
            if let Some(event) = self.notification.take() {
                match events.try_send(event) {
                    Ok(()) => waker(),
                    Err(TrySendError::Full(event)) => {
                        self.notification = Some(event);
                        waker();
                    }
                    Err(TrySendError::Disconnected(_)) => break,
                }
            }
            if self.status.sampling
                && self.control.sampling.load(Ordering::Acquire)
                && Instant::now() >= self.next_sample
            {
                self.sample();
                waker();
                self.next_sample = Instant::now()
                    + Duration::from_secs_f64(
                        speed.load(Ordering::Relaxed).max(1) as f64 * INTERVAL_STEP,
                    );
            }
            std::thread::sleep(TICK);
        }
        // Drop the pending child before Access, closing every lifetime pipe; no blocking wait.
        self.pending = None;
        self.access.stop_session();
        self.access.reap();
    }
}

#[cfg(test)]
#[path = "hardware_worker_tests.rs"]
mod tests;
