/* crates/mc-magpie/src/sockets.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Per-process TCP throughput without privileges, used when magpie has no per-process network
//! data (nethogs missing or without capture permissions).
//!
//! The kernel reports byte counters of every TCP socket to any user through netlink `sock_diag`
//! (`tcp_info.tcpi_bytes_acked` / `tcpi_bytes_received`, as shown by `ss -ti`). Sockets are matched
//! to processes through `/proc/<pid>/fd`, which is readable for the user's own processes only.
//! UDP sockets have no byte counters, so QUIC/HTTP3 traffic is not counted. Runs on the gatherer
//! thread.

use std::collections::{HashMap, HashSet};
use std::io;
use std::time::Instant;

use magpie_types::processes::Process;

const NETLINK_SOCK_DIAG: libc::c_int = 4;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const INET_DIAG_INFO: u16 = 2;
const NLMSG_HDR: usize = 16;
/// `struct inet_diag_msg`
const DIAG_MSG_LEN: usize = 72;
const DIAG_MSG_UID: usize = 64;
const DIAG_MSG_INODE: usize = 68;
/// Offsets in `struct tcp_info` (stable ABI, Linux ≥ 4.2)
const TCPI_BYTES_ACKED: usize = 120;
const TCPI_BYTES_RECEIVED: usize = 128;

/// `inet_diag_sockid` inside `inet_diag_msg`: source port (big endian) and source and destination
/// addresses (16 bytes each)
const DIAG_MSG_SPORT: usize = 4;
const DIAG_MSG_SRC: usize = 8;
const TCP_LISTEN: u8 = 10;
const DIAG_MSG_DST: usize = 24;

/// Counters of one TCP socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpSocket {
    pub inode: u64,
    pub uid: u32,
    pub sent: u64,
    pub received: u64,
    /// Both ends on this machine (127.0.0.0/8, ::1, ::ffff:127.0.0.0/104)
    pub loopback: bool,
    pub listening: bool,
    pub local_port: u16,
}

fn is_loopback(family: u8, addr: &[u8]) -> bool {
    if family == libc::AF_INET as u8 {
        addr[0] == 127
    } else {
        addr[..15].iter().all(|b| *b == 0) && addr[15] == 1
            || addr[..10].iter().all(|b| *b == 0)
                && addr[10] == 0xff
                && addr[11] == 0xff
                && addr[12] == 127
    }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    buf.get(at..at + 4)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
}

fn u64_at(buf: &[u8], at: usize) -> Option<u64> {
    buf.get(at..at + 8)
        .map(|b| u64::from_ne_bytes(b.try_into().unwrap()))
}

fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    buf.get(at..at + 2)
        .map(|b| u16::from_ne_bytes(b.try_into().unwrap()))
}

/// Parses one netlink datagram of `SOCK_DIAG_BY_FAMILY` replies. Returns `true` at `NLMSG_DONE`.
pub fn parse_replies(buf: &[u8], out: &mut Vec<TcpSocket>) -> io::Result<bool> {
    let mut at = 0;
    while at + NLMSG_HDR <= buf.len() {
        let len = u32_at(buf, at).unwrap_or(0) as usize;
        let kind = u16_at(buf, at + 4).unwrap_or(0);
        if len < NLMSG_HDR || at + len > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated netlink message",
            ));
        }
        match kind {
            NLMSG_DONE => return Ok(true),
            NLMSG_ERROR => {
                let errno = u32_at(buf, at + NLMSG_HDR).unwrap_or(0) as i32;
                return Err(io::Error::from_raw_os_error(-errno));
            }
            SOCK_DIAG_BY_FAMILY => {
                let msg = &buf[at + NLMSG_HDR..at + len];
                if msg.len() >= DIAG_MSG_LEN {
                    let family = msg[0];
                    let src = &msg[DIAG_MSG_SRC..DIAG_MSG_SRC + 16];
                    let dst = &msg[DIAG_MSG_DST..DIAG_MSG_DST + 16];
                    let mut socket = TcpSocket {
                        inode: u32_at(msg, DIAG_MSG_INODE).unwrap_or(0) as u64,
                        uid: u32_at(msg, DIAG_MSG_UID).unwrap_or(0),
                        sent: 0,
                        received: 0,
                        loopback: is_loopback(family, src) && is_loopback(family, dst),
                        listening: msg[1] == TCP_LISTEN,
                        local_port: u16::from_be_bytes([
                            msg[DIAG_MSG_SPORT],
                            msg[DIAG_MSG_SPORT + 1],
                        ]),
                    };
                    let mut attr = align4(DIAG_MSG_LEN);
                    while attr + 4 <= msg.len() {
                        let attr_len = u16_at(msg, attr).unwrap_or(0) as usize;
                        let attr_kind = u16_at(msg, attr + 2).unwrap_or(0);
                        if attr_len < 4 || attr + attr_len > msg.len() {
                            break;
                        }
                        if attr_kind == INET_DIAG_INFO {
                            let info = &msg[attr + 4..attr + attr_len];
                            socket.sent = u64_at(info, TCPI_BYTES_ACKED).unwrap_or(0);
                            socket.received = u64_at(info, TCPI_BYTES_RECEIVED).unwrap_or(0);
                        }
                        attr += align4(attr_len);
                    }
                    if socket.inode != 0 {
                        out.push(socket);
                    }
                }
            }
            _ => {}
        }
        at += align4(len);
    }
    Ok(false)
}

/// All TCP sockets of one address family with their counters.
fn dump_tcp(family: u8, out: &mut Vec<TcpSocket>) -> io::Result<()> {
    // SAFETY: plain socket syscalls on a descriptor owned by this function
    unsafe {
        let fd = libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            NETLINK_SOCK_DIAG,
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            // nlmsghdr + inet_diag_req_v2 (family, protocol, ext, pad, states, inet_diag_sockid)
            let mut req = [0u8; NLMSG_HDR + 56];
            let req_len = req.len() as u32;
            req[0..4].copy_from_slice(&req_len.to_ne_bytes());
            req[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
            let flags = (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16;
            req[6..8].copy_from_slice(&flags.to_ne_bytes());
            req[8..12].copy_from_slice(&1u32.to_ne_bytes());
            req[16] = family;
            req[17] = libc::IPPROTO_TCP as u8;
            req[18] = 1 << (INET_DIAG_INFO - 1);
            req[20..24].copy_from_slice(&u32::MAX.to_ne_bytes());

            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as u16;
            let sent = libc::sendto(
                fd,
                req.as_ptr().cast(),
                req.len(),
                0,
                (&addr as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as u32,
            );
            if sent < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
                if n < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(err);
                }
                if n == 0 || parse_replies(&buf[..n as usize], out)? {
                    return Ok(());
                }
            }
        })();
        libc::close(fd);
        result
    }
}

/// Socket inode of every file descriptor seen so far, per process. Only new descriptor numbers
/// are resolved with `readlink`; the cache is dropped every `FD_CACHE_ROUNDS` scans so reused
/// descriptor numbers are picked up.
#[derive(Default)]
struct FdCache {
    /// pid → (fd → socket inode, 0 for other files)
    fds: HashMap<u32, HashMap<u32, u64>>,
    scans: u32,
}

const FD_CACHE_ROUNDS: u32 = 15;

fn socket_inode(link: &std::path::Path) -> u64 {
    link.to_str()
        .and_then(|t| t.strip_prefix("socket:["))
        .and_then(|t| t.strip_suffix(']'))
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(0)
}

impl FdCache {
    /// `socket inode → pid` for the sockets in `wanted`, scanning processes in `pids` order and
    /// stopping once all are found.
    fn find_owners(
        &mut self,
        pids: impl Iterator<Item = u32>,
        wanted: &HashSet<u64>,
        owners: &mut HashMap<u64, u32>,
    ) {
        self.scans += 1;
        if self.scans.is_multiple_of(FD_CACHE_ROUNDS) {
            self.fds.clear();
        }
        let mut missing = wanted.len();
        for pid in pids {
            if missing == 0 {
                break;
            }
            let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
                self.fds.remove(&pid);
                continue;
            };
            let cached = self.fds.entry(pid).or_default();
            let mut present = HashSet::with_capacity(cached.len());
            for entry in entries.flatten() {
                let Some(fd) = entry
                    .file_name()
                    .to_str()
                    .and_then(|n| n.parse::<u32>().ok())
                else {
                    continue;
                };
                present.insert(fd);
                let inode = *cached.entry(fd).or_insert_with(|| {
                    std::fs::read_link(entry.path()).map_or(0, |l| socket_inode(&l))
                });
                if inode != 0 && wanted.contains(&inode) && owners.insert(inode, pid).is_none() {
                    missing -= 1;
                }
            }
            cached.retain(|fd, _| present.contains(fd));
        }
    }

    fn retain(&mut self, alive: impl Fn(u32) -> bool) {
        self.fds.retain(|pid, _| alive(*pid));
    }
}

/// Turns socket counters into per-process rates between calls.
#[derive(Default)]
pub struct SocketTraffic {
    last: HashMap<u64, (u64, u64)>,
    owners: HashMap<u64, u32>,
    /// Sockets whose owner was not found (other users' or exited processes); not searched again
    unowned: HashSet<u64>,
    fd_cache: FdCache,
    last_time: Option<Instant>,
    uid: Option<u32>,
}

impl SocketTraffic {
    /// Sets `usage_stats.network_usage` (bytes/s) of `processes` from TCP sockets. Returns `false`
    /// when socket diagnostics are unavailable (the values are left untouched).
    pub fn sample(&mut self, processes: &mut HashMap<u32, Process>) -> bool {
        let now = Instant::now();
        let mut sockets = Vec::with_capacity(self.last.len().max(64));
        for family in [libc::AF_INET as u8, libc::AF_INET6 as u8] {
            if let Err(e) = dump_tcp(family, &mut sockets) {
                log::debug!(target: "TaskManager::Sockets", "sock_diag unavailable: {e}");
                return false;
            }
        }
        // SAFETY: getuid has no preconditions
        let uid = *self.uid.get_or_insert_with(|| unsafe { libc::getuid() });

        let elapsed = self.last_time.map(|t| now.duration_since(t).as_secs_f32());
        self.last_time = Some(now);

        // Loopback connections count for the client only: an app talking to a local proxy shows
        // its traffic, the proxy's accepting side does not count it again (its own outbound
        // connections carry the real network traffic)
        let listen_ports: HashSet<u16> = sockets
            .iter()
            .filter(|s| s.listening)
            .map(|s| s.local_port)
            .collect();

        // Bytes moved per socket since the previous call; sockets seen for the first time count
        // fully except on the first call (no baseline yet)
        let mut deltas: Vec<(u64, u64)> = vec![];
        let mut current = HashMap::with_capacity(sockets.len());
        for s in &sockets {
            let total = (s.sent, s.received);
            current.insert(s.inode, total);
            let server_side = s.loopback && listen_ports.contains(&s.local_port);
            if (uid != 0 && s.uid != uid) || server_side {
                continue;
            }
            let moved = match self.last.get(&s.inode) {
                Some(&(sent, received)) => {
                    s.sent.saturating_sub(sent) + s.received.saturating_sub(received)
                }
                None if elapsed.is_some() => s.sent + s.received,
                None => 0,
            };
            if moved > 0 {
                deltas.push((s.inode, moved));
            }
        }
        self.last = current;
        self.owners
            .retain(|inode, pid| self.last.contains_key(inode) && processes.contains_key(pid));
        self.unowned.retain(|inode| self.last.contains_key(inode));

        let Some(elapsed) = elapsed.filter(|e| *e > 0.) else {
            return true;
        };

        let unknown: HashSet<u64> = deltas
            .iter()
            .map(|(inode, _)| *inode)
            .filter(|inode| !self.owners.contains_key(inode) && !self.unowned.contains(inode))
            .collect();
        if !unknown.is_empty() {
            // Processes that owned sockets before are the likeliest owners of new ones
            let previous: HashSet<u32> = self.owners.values().copied().collect();
            let order = previous.iter().copied().chain(
                processes
                    .keys()
                    .copied()
                    .filter(|pid| !previous.contains(pid)),
            );
            self.fd_cache.retain(|pid| processes.contains_key(&pid));
            self.fd_cache.find_owners(order, &unknown, &mut self.owners);
            self.unowned.extend(
                unknown
                    .into_iter()
                    .filter(|inode| !self.owners.contains_key(inode)),
            );
        }

        let mut per_pid: HashMap<u32, u64> = HashMap::new();
        for (inode, moved) in deltas {
            if let Some(pid) = self.owners.get(&inode) {
                *per_pid.entry(*pid).or_default() += moved;
            }
        }
        for (pid, process) in processes.iter_mut() {
            process.usage_stats.network_usage =
                per_pid.get(pid).map_or(0., |b| *b as f32 / elapsed);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn reply(inode: u32, uid: u32, acked: u64, received: u64) -> Vec<u8> {
        let mut info = vec![0u8; 140];
        info[TCPI_BYTES_ACKED..TCPI_BYTES_ACKED + 8].copy_from_slice(&acked.to_ne_bytes());
        info[TCPI_BYTES_RECEIVED..TCPI_BYTES_RECEIVED + 8].copy_from_slice(&received.to_ne_bytes());
        let mut msg = vec![0u8; DIAG_MSG_LEN];
        msg[DIAG_MSG_UID..DIAG_MSG_UID + 4].copy_from_slice(&uid.to_ne_bytes());
        msg[DIAG_MSG_INODE..DIAG_MSG_INODE + 4].copy_from_slice(&inode.to_ne_bytes());
        msg.extend_from_slice(&((info.len() + 4) as u16).to_ne_bytes());
        msg.extend_from_slice(&INET_DIAG_INFO.to_ne_bytes());
        msg.extend_from_slice(&info);
        let mut out = vec![0u8; NLMSG_HDR];
        out[0..4].copy_from_slice(&((NLMSG_HDR + msg.len()) as u32).to_ne_bytes());
        out[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
        out.extend_from_slice(&msg);
        out
    }

    #[test]
    fn parses_counters_and_done() {
        let mut buf = reply(42, 1000, 1500, 9000);
        buf.extend(reply(43, 0, 1, 2));
        let mut done = vec![0u8; NLMSG_HDR + 4];
        done[0..4].copy_from_slice(&((NLMSG_HDR + 4) as u32).to_ne_bytes());
        done[4..6].copy_from_slice(&NLMSG_DONE.to_ne_bytes());
        buf.extend(done);
        let mut out = vec![];
        assert!(parse_replies(&buf, &mut out).unwrap());
        assert_eq!(
            out,
            [
                TcpSocket {
                    inode: 42,
                    uid: 1000,
                    sent: 1500,
                    received: 9000,
                    loopback: false,
                    listening: false,
                    local_port: 0
                },
                TcpSocket {
                    inode: 43,
                    uid: 0,
                    sent: 1,
                    received: 2,
                    loopback: false,
                    listening: false,
                    local_port: 0
                }
            ]
        );
    }

    /// End to end against the running kernel: traffic over a loopback TCP connection is counted
    /// once, for the client side (the accepting side is excluded).
    #[test]
    fn measures_own_loopback_traffic() {
        let pid = std::process::id();
        let mut processes = HashMap::new();
        processes.insert(
            pid,
            Process {
                pid,
                ..Default::default()
            },
        );

        let mut traffic = SocketTraffic::default();
        if !traffic.sample(&mut processes) {
            eprintln!("sock_diag unavailable here; skipping");
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let payload = vec![7u8; 256 * 1024];
        let reader = std::thread::spawn(move || {
            let mut buf = vec![0u8; payload.len()];
            server.read_exact(&mut buf).unwrap();
            buf.len()
        });
        client.write_all(&vec![7u8; 256 * 1024]).unwrap();
        assert_eq!(reader.join().unwrap(), 256 * 1024);
        std::thread::sleep(std::time::Duration::from_millis(50));

        assert!(traffic.sample(&mut processes));
        let rate = processes[&pid].usage_stats.network_usage;
        assert!(rate > 0., "loopback traffic not attributed: {rate}");
    }

    #[test]
    fn loopback_addresses() {
        let v4 = |a: [u8; 4]| {
            let mut b = [0u8; 16];
            b[..4].copy_from_slice(&a);
            b
        };
        assert!(is_loopback(libc::AF_INET as u8, &v4([127, 0, 0, 1])));
        assert!(!is_loopback(libc::AF_INET as u8, &v4([192, 168, 1, 2])));
        let mut v6 = [0u8; 16];
        v6[15] = 1;
        assert!(is_loopback(libc::AF_INET6 as u8, &v6));
        let mut mapped = [0u8; 16];
        mapped[10] = 0xff;
        mapped[11] = 0xff;
        mapped[12] = 127;
        mapped[15] = 1;
        assert!(is_loopback(libc::AF_INET6 as u8, &mapped));
        let mut global = [0u8; 16];
        global[0] = 0x20;
        assert!(!is_loopback(libc::AF_INET6 as u8, &global));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Prints the busiest processes of this machine and the sampling cost (manual check).
    #[test]
    #[ignore]
    fn live_top_processes() {
        let mut processes: HashMap<u32, Process> = std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .map(|pid| {
                let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
                (
                    pid,
                    Process {
                        pid,
                        name: name.trim().into(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let mut traffic = SocketTraffic::default();
        assert!(traffic.sample(&mut processes));
        for round in 0..3 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let started = Instant::now();
            traffic.sample(&mut processes);
            let cost = started.elapsed();
            let mut top: Vec<_> = processes
                .values()
                .filter(|p| p.usage_stats.network_usage > 0.)
                .collect();
            top.sort_by(|a, b| {
                b.usage_stats
                    .network_usage
                    .total_cmp(&a.usage_stats.network_usage)
            });
            println!("round {round}: sample took {cost:?}");
            for p in top.iter().take(6) {
                println!(
                    "  {:>8} {:<20} {:>10.0} B/s",
                    p.pid, p.name, p.usage_stats.network_usage
                );
            }
        }
    }
}

#[cfg(test)]
mod profile {
    use super::*;

    #[test]
    #[ignore]
    fn profile_parts() {
        let mut cache = FdCache::default();
        for _ in 0..3 {
            let t = Instant::now();
            let mut sockets = vec![];
            dump_tcp(libc::AF_INET as u8, &mut sockets).unwrap();
            dump_tcp(libc::AF_INET6 as u8, &mut sockets).unwrap();
            let dump = t.elapsed();
            let t = Instant::now();
            let wanted: HashSet<u64> = sockets.iter().map(|s| s.inode).collect();
            let pids: Vec<u32> = std::fs::read_dir("/proc")
                .unwrap()
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect();
            let mut owners = HashMap::new();
            cache.find_owners(pids.iter().copied(), &wanted, &mut owners);
            println!(
                "{} sockets, dump {dump:?}; owners {} of {}, scan {:?}",
                sockets.len(),
                owners.len(),
                wanted.len(),
                t.elapsed()
            );
        }
    }
}
