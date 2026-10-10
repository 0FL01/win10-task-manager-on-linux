/* crates/mc-magpie/src/users.rs
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

//! Process owners for the Details tab: the real uid from `/proc/<pid>/status`, named through
//! `/etc/passwd`. Runs on the gatherer thread only; uids are cached per pid, so each cycle reads
//! the status file of new processes only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use magpie_types::processes::Process;

/// `uid → name` from passwd(5) text.
pub fn parse_passwd(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let uid = fields.nth(1)?.parse().ok()?;
            (!name.is_empty()).then(|| (uid, name.to_owned()))
        })
        .collect()
}

/// Real uid from the text of `/proc/<pid>/status`.
pub fn parse_status_uid(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

pub struct UserResolver {
    root: PathBuf,
    names: HashMap<u32, String>,
    passwd_mtime: Option<SystemTime>,
    uids: HashMap<u32, u32>,
}

impl Default for UserResolver {
    fn default() -> Self {
        Self::with_root("/")
    }
}

impl UserResolver {
    /// `root` replaces `/` (tests).
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            names: HashMap::new(),
            passwd_mtime: None,
            uids: HashMap::new(),
        }
    }

    fn refresh_passwd(&mut self) {
        let path = self.root.join("etc/passwd");
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime.is_some() && mtime == self.passwd_mtime {
            return;
        }
        self.passwd_mtime = mtime;
        self.names = std::fs::read_to_string(&path)
            .map(|t| parse_passwd(&t))
            .unwrap_or_default();
    }

    /// `pid → user name` for `processes`; processes whose status cannot be read are left out.
    pub fn resolve(&mut self, processes: &HashMap<u32, Process>) -> HashMap<u32, String> {
        self.refresh_passwd();
        self.uids.retain(|pid, _| processes.contains_key(pid));
        let mut out = HashMap::with_capacity(processes.len());
        for &pid in processes.keys() {
            let uid = match self.uids.get(&pid) {
                Some(uid) => *uid,
                None => {
                    let status = self.root.join(format!("proc/{pid}/status"));
                    let Some(uid) = std::fs::read_to_string(status)
                        .ok()
                        .as_deref()
                        .and_then(parse_status_uid)
                    else {
                        continue;
                    };
                    self.uids.insert(pid, uid);
                    uid
                }
            };
            let name = self
                .names
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| uid.to_string());
            out.insert(pid, name);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd_and_status_parsing() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n# comment\nalice:x:1000:1000::/home/alice:/bin/zsh\nbroken\n";
        let names = parse_passwd(passwd);
        assert_eq!(names[&0], "root");
        assert_eq!(names[&1000], "alice");
        assert_eq!(names.len(), 2);
        assert_eq!(
            parse_status_uid("Name:\tbash\nUid:\t1000\t1000\t1000\t1000\n"),
            Some(1000)
        );
        assert_eq!(parse_status_uid("Name:\tbash\n"), None);
    }

    #[test]
    fn resolves_from_a_fake_root() {
        let root = std::env::temp_dir().join(format!("mc-users-{}", std::process::id()));
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(root.join("proc/42")).unwrap();
        std::fs::write(
            root.join("etc/passwd"),
            "bob:x:1001:1001::/home/bob:/bin/sh\n",
        )
        .unwrap();
        std::fs::write(
            root.join("proc/42/status"),
            "Name:\tx\nUid:\t1001\t1001\t1001\t1001\n",
        )
        .unwrap();
        let mut processes = HashMap::new();
        processes.insert(42, Process::default());
        processes.insert(43, Process::default());
        let mut resolver = UserResolver::with_root(&root);
        let users = resolver.resolve(&processes);
        assert_eq!(users.get(&42).map(String::as_str), Some("bob"));
        assert!(!users.contains_key(&43), "unreadable status is left out");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
