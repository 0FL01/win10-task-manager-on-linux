/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
use super::*;
use std::process::{Command, Stdio};

fn worker() -> Worker {
    let control = Arc::new(Control::new(Arc::new(AtomicBool::new(true))));
    let mut access = Access::default();
    access.permanent = std::env::temp_dir().join(format!("tm-absent-{}", rand::random::<u64>()));
    Worker {
        access,
        available: true,
        control,
        samples: Arc::new(LatestSlot::new()),
        status: HardwareStatus::default(),
        pending: None,
        notification: None,
        next_sample: Instant::now(),
    }
}
fn child() -> Child {
    Command::new("/usr/bin/python3")
        .args(["-I", "-c", "import sys; sys.stdin.buffer.read()"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap()
}
fn pending(child: Child) -> Pending {
    Pending {
        id: 17,
        operation: HardwareOperation::Install,
        deadline: Instant::now() + AUTH_TIMEOUT,
        child: Some(child),
        socket: None,
        cancelling: false,
        completion: None,
    }
}
fn poll_finished(worker: &mut Worker, pending: &mut Pending) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while pending.completion.is_none() {
        worker.poll(pending);
        assert!(Instant::now() < deadline);
        std::thread::sleep(TICK);
    }
}
#[test]
fn saturated_event_channel_retains_completion_and_rejects_another_job() {
    let running = Arc::new(AtomicBool::new(true));
    let (sender, receiver) = mpsc::sync_channel(1);
    sender
        .try_send(Event::About(crate::About::default()))
        .unwrap();
    let handle = Handle::spawn(
        false,
        running,
        Arc::new(AtomicU64::new(20)),
        sender,
        Arc::new(|| {}),
    );
    assert!(handle.submit(1, HardwareOperation::Enable { interactive: false }));
    std::thread::sleep(TICK * 3);
    assert!(!handle.submit(2, HardwareOperation::Spd));
    assert!(matches!(receiver.try_recv().unwrap(), Event::About(_)));
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        Event::HardwareAccess {
            id: Some(1),
            result: Err(_),
            ..
        }
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !handle.submit(2, HardwareOperation::Spd) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(TICK);
    }
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        Event::Spd {
            id: 2,
            result: Err(_),
            ..
        }
    ));
    assert!(receiver.try_recv().is_err());
}
#[test]
fn shutdown_does_not_wait_for_a_full_event_channel() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    sender
        .try_send(Event::About(crate::About::default()))
        .unwrap();
    let handle = Handle::spawn(
        false,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(20)),
        sender,
        Arc::new(|| {}),
    );
    assert!(handle.submit(1, HardwareOperation::Spd));
    std::thread::sleep(TICK * 2);
    let started = Instant::now();
    drop(handle);
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn sampling_failure_invalidates_without_acknowledging_install_or_revoking_grant() {
    for mode in [HardwareMode::Temporary, HardwareMode::Persistent] {
        let mut worker = worker();
        worker.status = HardwareStatus {
            mode,
            sampling: true,
            grant: HardwareGrant::Present,
            epoch: 0,
        };
        worker.control.sampling.store(true, Ordering::Release);
        if mode == HardwareMode::Temporary {
            worker
                .access
                .adopt_session(child(), worker.access.permanent.clone());
        }
        worker.sample();
        assert!(!worker.status.sampling);
        assert_eq!(worker.status.grant, HardwareGrant::Present);
        assert_eq!(
            worker.status.mode,
            if mode == HardwareMode::Temporary {
                HardwareMode::Off
            } else {
                HardwareMode::Persistent
            }
        );
        assert!(worker.samples.take().unwrap().data.is_none());
        assert!(matches!(
            worker.notification,
            Some(Event::HardwareAccess { id: None, .. })
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.access.reap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(TICK);
        }
    }
}
#[test]
fn received_denial_is_applied_even_when_sampling_was_interrupted() {
    let mut worker = worker();
    worker.status = HardwareStatus {
        mode: HardwareMode::Persistent,
        sampling: true,
        grant: HardwareGrant::Present,
        epoch: 0,
    };
    worker.control.sampling.store(true, Ordering::Release);
    worker.control.active.store(5, Ordering::Release);
    worker.control.cancelled.store(5, Ordering::Release);
    assert!(worker.control.sampling_interrupted());
    worker.apply_sample(Err("Sampling interrupted for hardware control.".into()));
    assert!(worker.status.sampling);
    assert!(worker.notification.is_none());
    assert!(worker.samples.take().is_none());
    let denial: Reply = serde_json::from_str(r#"{"enabled":false,"revoked":true}"#).unwrap();
    worker.apply_sample(Ok(denial));
    assert_eq!(worker.status.grant, HardwareGrant::Absent);
    assert!(!worker.status.sampling);
    assert!(!worker.control.sampling.load(Ordering::Acquire));
    assert!(worker.status.epoch > 0);
    assert!(worker.samples.take().unwrap().data.is_none());
    assert!(matches!(
        worker.notification,
        Some(Event::HardwareAccess {
            id: None,
            result: Err(_),
            ..
        })
    ));
}
#[test]
fn failed_permanent_verification_preserves_temporary_session() {
    let mut worker = worker();
    worker.status.mode = HardwareMode::Temporary;
    worker.status.sampling = true;
    worker
        .access
        .adopt_session(child(), worker.access.permanent.clone());
    let mut pending = pending(Command::new("/usr/bin/true").spawn().unwrap());
    poll_finished(&mut worker, &mut pending);
    assert!(matches!(
        pending.completion,
        Some(Event::HardwareAccess { result: Err(_), .. })
    ));
    assert_eq!(worker.status.mode, HardwareMode::Temporary);
    assert!(worker.status.sampling);
    assert!(worker.access.session_owned());
}
#[test]
fn failed_authorization_is_terminal_without_starting_a_fallback() {
    let mut worker = worker();
    let mut pending = pending(Command::new("/usr/bin/false").spawn().unwrap());
    pending.operation = HardwareOperation::Enable { interactive: true };
    poll_finished(&mut worker, &mut pending);
    assert!(pending.child.is_none());
    assert!(pending.socket.is_none());
    assert!(!worker.access.session_owned());
    assert!(matches!(
        pending.completion,
        Some(Event::HardwareAccess { result: Err(_), .. })
    ));
}
#[test]
fn cancelled_child_is_quiescent_before_terminal_completion() {
    let mut worker = worker();
    let mut pending = pending(child());
    worker.control.cancelled.store(17, Ordering::Release);
    poll_finished(&mut worker, &mut pending);
    assert!(pending.cancelling);
    assert!(pending.child.is_none());
    assert_eq!(worker.status.grant, HardwareGrant::Unconfirmed);
    assert!(matches!(
        pending.completion,
        Some(Event::HardwareAccess {
            id: Some(17),
            result: Err(_),
            ..
        })
    ));
}
#[cfg(target_os = "linux")]
#[test]
fn permanent_endpoint_is_verified_before_switching_and_revoke_needs_explicit_ack() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    let path = std::env::temp_dir().join(format!(
        "tm-worker-{}-{}.sock",
        std::process::id(),
        rand::random::<u64>()
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        for response in [
            b"{\"enabled\":true}\n".as_slice(),
            b"{\"enabled\":false}\n".as_slice(),
            b"{\"enabled\":false,\"revoked\":true}\n".as_slice(),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 64];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream.write_all(response).unwrap();
        }
    });
    let mut worker = worker();
    worker.access.permanent = path.clone();
    worker.status.mode = HardwareMode::Temporary;
    worker.status.sampling = true;
    worker
        .access
        .adopt_session(child(), path.with_extension("temporary.sock"));
    let mut install = pending(Command::new("/usr/bin/true").spawn().unwrap());
    poll_finished(&mut worker, &mut install);
    assert_eq!(worker.status.mode, HardwareMode::Persistent);
    assert_eq!(worker.status.grant, HardwareGrant::Present);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.access.reap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(TICK);
    }
    let mut disable = worker.begin(18, HardwareOperation::Disable);
    worker.poll(&mut disable);
    assert_eq!(worker.status.grant, HardwareGrant::Unconfirmed);
    assert!(matches!(
        disable.completion,
        Some(Event::HardwareAccess { result: Err(_), .. })
    ));
    let mut retry = worker.begin(19, HardwareOperation::Disable);
    worker.poll(&mut retry);
    assert_eq!(worker.status.grant, HardwareGrant::Absent);
    assert!(matches!(
        retry.completion,
        Some(Event::HardwareAccess { result: Ok(()), .. })
    ));
    server.join().unwrap();
    std::fs::remove_file(path).unwrap();
}
