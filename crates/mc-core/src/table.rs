/* crates/mc-core/src/table.rs
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

//! Tree model behind the Apps and Services tables, ported from the GTK `table_view/models.rs`.
//!
//! A [`Tree`] is rebuilt from scratch for each snapshot (cheap: a few thousand nodes at most)
//! and turned into a flat list of visible rows with [`Tree::flatten`]. Sorting, filtering and
//! flattening only happen when the data, the sort order, the search text or the expanded set
//! change; the UI then renders just the rows in the viewport.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use mc_magpie::magpie_types::processes::ProcessUsageStats;
use mc_magpie::{App, Process, Readings, Service};
use textdistance::nstr::levenshtein;

use crate::config::SortOrder;
use crate::gpu::{GpuSnapshot, GpuStats};
use crate::i18n::i18n;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowKind {
    SectionHeader,
    App,
    Process,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowIcon {
    /// Generic executable icon
    Default,
    /// Icon of the app with this id (resolved through the app icon cache)
    App(String),
    /// One of the bundled service state icons
    Service(ServiceIcon),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceIcon {
    Running,
    Failed,
    Stopped,
    Disabled,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServiceInfo {
    pub id: u64,
    pub running: bool,
    pub enabled: bool,
    pub failed: bool,
    pub user: String,
    pub group: String,
    pub file_path: String,
    pub description: String,
}

impl ServiceInfo {
    pub fn stopped(&self) -> bool {
        !self.running && !self.failed && self.enabled
    }

    pub fn disabled(&self) -> bool {
        !self.enabled && !self.running && !self.failed
    }

    pub fn icon(&self) -> ServiceIcon {
        if self.running {
            ServiceIcon::Running
        } else if self.failed {
            ServiceIcon::Failed
        } else if self.enabled {
            ServiceIcon::Stopped
        } else {
            ServiceIcon::Disabled
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    /// Stable identity used to remember expansion and selection across snapshots.
    pub key: String,
    pub kind: RowKind,
    pub section: u8,
    pub name: String,
    pub pid: u32,
    pub command_line: String,
    pub icon: RowIcon,
    pub stats: ProcessUsageStats,
    pub gpu: GpuStats,
    pub service: Option<ServiceInfo>,
    /// For app rows: every pid belonging to the app.
    pub app_pids: Vec<u32>,
    pub children: Vec<usize>,
}

impl Node {
    fn new(key: String, kind: RowKind, section: u8, name: String) -> Self {
        Self {
            key,
            kind,
            section,
            name,
            pid: 0,
            command_line: String::new(),
            icon: RowIcon::None,
            stats: ProcessUsageStats::default(),
            gpu: GpuStats::default(),
            service: None,
            app_pids: vec![],
            children: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Pid,
    Cpu,
    Memory,
    Swap,
    Drive,
    Network,
    Gpu,
    GpuEngine,
    GpuMemory,
    GpuDedicatedMemory,
    GpuSharedMemory,
}

impl Column {
    pub const ALL: [Column; 12] = [
        Column::Name,
        Column::Pid,
        Column::Cpu,
        Column::Memory,
        Column::Swap,
        Column::Drive,
        Column::Network,
        Column::Gpu,
        Column::GpuEngine,
        Column::GpuMemory,
        Column::GpuDedicatedMemory,
        Column::GpuSharedMemory,
    ];

    /// Identifier persisted in the settings (same as the GTK column ids).
    pub fn id(&self) -> &'static str {
        match self {
            Column::Name => "name",
            Column::Pid => "pid",
            Column::Cpu => "cpu",
            Column::Memory => "memory",
            Column::Swap => "swap",
            Column::Drive => "drive",
            Column::Network => "network",
            Column::Gpu => "gpu",
            Column::GpuEngine => "gpu_engine",
            Column::GpuMemory => "gpu_memory",
            Column::GpuDedicatedMemory => "dedicated_gpu_memory",
            Column::GpuSharedMemory => "shared_gpu_memory",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Value used for "hide when zero" dimming.
    pub fn numeric_value(&self, node: &Node) -> Option<f64> {
        let s = &node.stats;
        Some(match self {
            Column::Cpu => s.cpu_usage as f64,
            Column::Memory => s.memory_usage as f64,
            Column::Swap => s.swap_usage as f64,
            Column::Drive => s.disk_usage as f64,
            Column::Network => s.network_usage as f64,
            Column::Gpu => node.gpu.usage.percent? as f64,
            Column::GpuMemory => node.gpu.memory? as f64,
            Column::GpuDedicatedMemory => node.gpu.dedicated? as f64,
            Column::GpuSharedMemory => node.gpu.shared? as f64,
            Column::Name | Column::Pid | Column::GpuEngine => return None,
        })
    }

    fn compare(&self, lhs: &Node, rhs: &Node) -> Ordering {
        let (l, r) = (&lhs.stats, &rhs.stats);
        match self {
            Column::Name => lhs.name.to_lowercase().cmp(&rhs.name.to_lowercase()),
            Column::Pid => lhs.pid.cmp(&rhs.pid),
            Column::Cpu => l
                .cpu_usage
                .partial_cmp(&r.cpu_usage)
                .unwrap_or(Ordering::Equal),
            Column::Memory => l.memory_usage.cmp(&r.memory_usage),
            Column::Swap => l.swap_usage.cmp(&r.swap_usage),
            Column::Drive => l
                .disk_usage
                .partial_cmp(&r.disk_usage)
                .unwrap_or(Ordering::Equal),
            Column::Network => l
                .network_usage
                .partial_cmp(&r.network_usage)
                .unwrap_or(Ordering::Equal),
            Column::Gpu => lhs
                .gpu
                .usage
                .percent
                .partial_cmp(&rhs.gpu.usage.percent)
                .unwrap_or(Ordering::Equal),
            Column::GpuEngine => lhs.gpu.usage.engine.cmp(&rhs.gpu.usage.engine),
            Column::GpuMemory => lhs.gpu.memory.cmp(&rhs.gpu.memory),
            Column::GpuDedicatedMemory => lhs.gpu.dedicated.cmp(&rhs.gpu.dedicated),
            Column::GpuSharedMemory => lhs.gpu.shared.cmp(&rhs.gpu.shared),
        }
    }

    fn gpu_known(&self, node: &Node) -> Option<bool> {
        match self {
            Self::GpuEngine => Some(node.gpu.usage.engine.is_some()),
            Self::Gpu | Self::GpuMemory | Self::GpuDedicatedMemory | Self::GpuSharedMemory => {
                Some(self.numeric_value(node).is_some())
            }
            _ => None,
        }
    }
}

/// Parses a `;` separated column order; unknown ids are skipped, missing columns appended.
pub fn parse_column_order(raw: &str) -> Vec<Column> {
    let mut order = raw
        .split(';')
        .filter_map(Column::from_id)
        .fold(Vec::new(), |mut acc, c| {
            if !acc.contains(&c) {
                acc.push(c);
            }
            acc
        });
    for c in Column::ALL {
        if !order.contains(&c) {
            order.push(c);
        }
    }
    order
}

pub fn serialize_column_order(order: &[Column]) -> String {
    order.iter().map(|c| c.id()).collect::<Vec<_>>().join(";")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServiceFilter {
    pub running: bool,
    pub failed: bool,
    pub stopped: bool,
    pub disabled: bool,
}

impl ServiceFilter {
    fn active(&self) -> bool {
        self.running || self.failed || self.stopped || self.disabled
    }

    fn matches(&self, info: &ServiceInfo) -> bool {
        (self.running && info.running)
            || (self.failed && info.failed)
            || (self.stopped && info.stopped())
            || (self.disabled && info.disabled())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewOptions {
    pub sort_column: Option<Column>,
    pub sort_order: SortOrder,
    pub search: String,
    pub service_filter: ServiceFilter,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            sort_column: None,
            sort_order: SortOrder::None,
            search: String::new(),
            service_filter: ServiceFilter::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    pub node: usize,
    pub depth: u16,
    pub has_children: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
}

/// Mirrors the "pretty name" computation of the GTK process rows.
pub fn pretty_process_name(process: &Process) -> String {
    let name = if process.exe.is_empty() {
        if let Some(cmd) = process.cmd.first() {
            let mut cmd = cmd
                .split_ascii_whitespace()
                .next()
                .and_then(|s| s.split('/').next_back())
                .unwrap_or(&process.name);
            if let Some(s) = cmd.strip_suffix(':') {
                cmd = s;
            }
            cmd.trim()
        } else {
            process.name.trim()
        }
    } else {
        let exe_name = process.exe.split('/').next_back().unwrap_or(&process.name);
        if exe_name.starts_with("wine") {
            if process.cmd.is_empty() {
                process.name.trim()
            } else {
                process.cmd[0]
                    .split('\\')
                    .next_back()
                    .unwrap_or(&process.name)
                    .split('/')
                    .next_back()
                    .unwrap_or(&process.name)
                    .trim()
            }
        } else {
            exe_name.trim()
        }
    };

    name.to_owned()
}

pub fn primary_processes(app: &App, process_map: &HashMap<u32, Process>) -> Vec<u32> {
    let mut secondary_processes = HashSet::new();
    for app_pid in app.pids.iter() {
        if let Some(process) = process_map.get(app_pid) {
            for child in &process.children {
                if app.pids.contains(child) {
                    secondary_processes.insert(*child);
                }
            }
        }
    }

    let mut primary = app
        .pids
        .iter()
        .copied()
        .filter(|pid| !secondary_processes.contains(pid))
        .collect::<Vec<_>>();

    if primary.is_empty() {
        for (index, pid) in app.pids.iter().enumerate() {
            if let Some(process) = process_map.get(pid)
                && (!process.children.is_empty() || index == app.pids.len() - 1)
            {
                primary.push(*pid);
                break;
            }
        }
    }

    primary.sort_unstable();
    primary.dedup();
    primary
}

impl Tree {
    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    #[allow(clippy::too_many_arguments)]
    fn add_process(
        &mut self,
        process_map: &HashMap<u32, Process>,
        pid: u32,
        parent_key: &str,
        section: u8,
        inherited_icon: &RowIcon,
        app_icons: &HashMap<u32, String>,
        use_merged_stats: bool,
        depth_guard: u16,
    ) -> Option<usize> {
        let process = process_map.get(&pid)?;
        if depth_guard > 512 {
            return None;
        }

        let key = format!("{parent_key}/p{pid}");
        let mut node = Node::new(
            key.clone(),
            RowKind::Process,
            section,
            pretty_process_name(process),
        );
        node.pid = pid;
        node.command_line = process.cmd.join(" ");
        node.stats = if use_merged_stats {
            process.merged_usage_stats(process_map)
        } else {
            process.usage_stats
        };
        node.icon = match app_icons.get(&pid) {
            Some(app_id) => RowIcon::App(app_id.clone()),
            None => inherited_icon.clone(),
        };

        let icon = node.icon.clone();
        let index = self.push(node);

        let mut children = process.children.clone();
        children.sort_unstable();
        for child in children {
            if let Some(c) = self.add_process(
                process_map,
                child,
                &key,
                section,
                &icon,
                app_icons,
                use_merged_stats,
                depth_guard + 1,
            ) {
                self.nodes[index].children.push(c);
            }
        }

        Some(index)
    }

    /// Builds the Apps page tree: an "Apps" section and a "Processes" section.
    pub fn apps(readings: &Readings, use_merged_stats: bool) -> Self {
        let mut tree = Tree::default();
        let process_map = &readings.running_processes;

        let apps_header = tree.push(Node::new(
            "apps".into(),
            RowKind::SectionHeader,
            0,
            i18n("Apps"),
        ));
        let procs_header = tree.push(Node::new(
            "processes".into(),
            RowKind::SectionHeader,
            1,
            i18n("Processes"),
        ));
        tree.roots = vec![apps_header, procs_header];

        let mut apps = readings.running_apps.values().collect::<Vec<_>>();
        apps.sort_by(|a, b| a.id.cmp(&b.id));

        // pid -> app id, for the icons of processes belonging to an app
        let mut app_icons = HashMap::new();
        let mut app_primary = Vec::with_capacity(apps.len());
        for app in &apps {
            let primary = primary_processes(app, process_map);
            for pid in &primary {
                app_icons.insert(*pid, app.id.clone());
            }
            app_primary.push(primary);
        }

        let root_pid = process_map.keys().min().copied().unwrap_or(1);
        if let Some(init) = process_map.get(&root_pid) {
            let mut children = init.children.clone();
            children.sort_unstable();
            for child in children {
                if let Some(c) = tree.add_process(
                    process_map,
                    child,
                    "processes",
                    1,
                    &RowIcon::Default,
                    &app_icons,
                    use_merged_stats,
                    0,
                ) {
                    tree.nodes[procs_header].children.push(c);
                }
            }
        }

        for (app, primary) in apps.into_iter().zip(app_primary) {
            if primary.is_empty() {
                log::debug!(target: "TaskManager::AppsPage", "Failed to find primary PID for app {}", app.name);
                continue;
            }

            let key = format!("apps/a{}", app.id);
            let mut node = Node::new(key.clone(), RowKind::App, 0, app.name.clone());
            node.icon = RowIcon::App(app.id.clone());
            node.command_line = app.command.clone();
            node.app_pids = app.pids.clone();
            node.pid = primary[0];

            let mut usage_stats = ProcessUsageStats::default();
            for process in primary.iter().filter_map(|pid| process_map.get(pid)) {
                usage_stats.merge(&process.merged_usage_stats(process_map));
            }
            node.stats = usage_stats;

            let index = tree.push(node);
            for pid in &primary {
                if let Some(c) = tree.add_process(
                    process_map,
                    *pid,
                    &key,
                    0,
                    &RowIcon::App(app.id.clone()),
                    &app_icons,
                    use_merged_stats,
                    0,
                ) {
                    tree.nodes[index].children.push(c);
                }
            }
            tree.nodes[apps_header].children.push(index);
        }

        tree
    }

    /// Builds the Services page tree: "User Services" and "System Services" sections.
    pub fn services(readings: &Readings, use_merged_stats: bool) -> Self {
        let mut tree = Tree::default();
        let process_map = &readings.running_processes;

        let user_header = tree.push(Node::new(
            "user".into(),
            RowKind::SectionHeader,
            0,
            i18n("User Services"),
        ));
        let system_header = tree.push(Node::new(
            "system".into(),
            RowKind::SectionHeader,
            1,
            i18n("System Services"),
        ));
        tree.roots = vec![user_header, system_header];

        for (header, section, prefix, services) in [
            (user_header, 0u8, "user", &readings.user_services),
            (system_header, 1u8, "system", &readings.system_services),
        ] {
            let mut services = services.values().collect::<Vec<&Service>>();
            services.sort_by(|a, b| a.name.cmp(&b.name));

            for service in services {
                let key = format!("{prefix}/s{}", service.id);
                let mut node =
                    Node::new(key.clone(), RowKind::Service, section, service.name.clone());
                let info = ServiceInfo {
                    id: service.id,
                    running: service.running,
                    enabled: service.enabled,
                    failed: service.failed,
                    user: service.user.clone().unwrap_or_default(),
                    group: service.group.clone().unwrap_or_default(),
                    file_path: service.file_path.clone().unwrap_or_default(),
                    description: service.description.clone().unwrap_or_default(),
                };
                node.icon = RowIcon::Service(info.icon());
                node.pid = service.pid.unwrap_or_default();
                node.service = Some(info.clone());

                if let Some(process) = service.pid.and_then(|pid| process_map.get(&pid)) {
                    node.stats = process.merged_usage_stats(process_map);
                }

                let index = tree.push(node);
                if let Some(pid) = service.pid
                    && let Some(c) = tree.add_process(
                        process_map,
                        pid,
                        &key,
                        section,
                        &RowIcon::Default,
                        &HashMap::new(),
                        use_merged_stats,
                        0,
                    )
                {
                    tree.nodes[c].service = Some(info);
                    tree.nodes[index].children.push(c);
                }
                tree.nodes[header].children.push(index);
            }
        }

        tree
    }

    pub fn find_by_key(&self, key: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.key == key)
    }

    /// GPU groups use unique process/client identities, independently of the legacy scalar stats.
    pub fn apply_gpu(&mut self, readings: &Readings, gpu: &GpuSnapshot, merged: bool) {
        for node in &mut self.nodes {
            if node.kind == RowKind::SectionHeader {
                continue;
            }
            let descend = node.kind != RowKind::Process || merged;
            let mut pending = if node.kind == RowKind::App {
                node.app_pids.clone()
            } else {
                vec![node.pid]
            };
            let mut pids = HashSet::new();
            while let Some(pid) = pending.pop() {
                if pids.insert(pid)
                    && descend
                    && let Some(process) = readings.running_processes.get(&pid)
                {
                    pending.extend(process.children.iter().copied());
                }
            }
            node.gpu = gpu.aggregate(pids);
        }
    }

    fn matches_search(node: &Node, query: &str) -> bool {
        if query.is_empty() || node.kind == RowKind::SectionHeader {
            return true;
        }

        let entry_name = node.name.to_lowercase();
        let pid = node.pid.to_string();

        if entry_name.contains(query) || pid.contains(query) {
            return true;
        }

        if query.contains(&entry_name) || query.contains(&pid) {
            return true;
        }

        levenshtein(&entry_name, query) <= 0.6
    }

    fn sort_children(&self, children: &mut [usize], options: &ViewOptions) {
        let Some(column) = options.sort_column else {
            return;
        };
        let descending = match options.sort_order {
            SortOrder::Ascending => false,
            SortOrder::Descending => true,
            SortOrder::None => return,
        };

        children.sort_by(|&a, &b| {
            let (lhs, rhs) = (&self.nodes[a], &self.nodes[b]);
            if lhs.section == rhs.section
                && lhs.kind == rhs.kind
                && let (Some(l), Some(r)) = (column.gpu_known(lhs), column.gpu_known(rhs))
                && l != r
            {
                return r.cmp(&l); // Unknown stays last in either direction.
            }
            let ordering = lhs
                .section
                .cmp(&rhs.section)
                .then(lhs.kind.cmp(&rhs.kind))
                .then_with(|| column.compare(lhs, rhs))
                .then(lhs.pid.cmp(&rhs.pid));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    /// Produces the rows to display, depth first.
    ///
    /// Without a search, children of collapsed rows are skipped. With a search, every node of the
    /// tree is tested and matches are shown (with their depth) regardless of expansion.
    pub fn flatten(&self, expanded: &HashSet<String>, options: &ViewOptions) -> Vec<VisibleRow> {
        let query = options.search.trim().to_lowercase();
        let mut out = Vec::with_capacity(self.nodes.len().min(4096));

        let mut roots = self.roots.clone();
        roots.sort_by_key(|&r| self.nodes[r].section);

        for root in roots {
            self.flatten_into(root, 0, expanded, options, &query, &mut out);
        }

        out
    }

    fn flatten_into(
        &self,
        index: usize,
        depth: u16,
        expanded: &HashSet<String>,
        options: &ViewOptions,
        query: &str,
        out: &mut Vec<VisibleRow>,
    ) {
        let node = &self.nodes[index];
        let searching = !query.is_empty();

        let passes_filter = node.kind == RowKind::SectionHeader
            || !options.service_filter.active()
            || node
                .service
                .as_ref()
                .is_some_and(|s| options.service_filter.matches(s));

        let is_expanded = node.kind == RowKind::SectionHeader
            && !expanded.contains(&format!("!{}", node.key))
            || expanded.contains(&node.key);

        if passes_filter && Self::matches_search(node, query) {
            out.push(VisibleRow {
                node: index,
                depth,
                has_children: !node.children.is_empty(),
                expanded: is_expanded || searching,
            });
        } else if !searching {
            return;
        }

        if !passes_filter {
            return;
        }

        if is_expanded || searching {
            let mut children = node.children.clone();
            self.sort_children(&mut children, options);
            for child in children {
                self.flatten_into(child, depth + 1, expanded, options, query, out);
            }
        }
    }
}

/// Expansion state keyed by [`Node::key`]. Section headers are expanded by default; collapsing
/// one stores a `!key` marker.
pub fn toggle_expanded(expanded: &mut HashSet<String>, node: &Node) {
    if node.kind == RowKind::SectionHeader {
        let marker = format!("!{}", node.key);
        if !expanded.remove(&marker) {
            expanded.insert(marker);
        }
    } else if !expanded.remove(&node.key) {
        expanded.insert(node.key.clone());
    }
}

/// Totals shown under the column titles, as in the GTK `update_column_titles`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnTotals {
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub drive_percent: f32,
    pub swap_percent: f32,
    pub network_bytes_ps: f32,
    /// Send + receive rate of the physical interfaces (bridges, Docker, VPN and virtual
    /// interfaces repeat traffic of the physical ones); available without per-process data
    pub system_network_bytes_ps: f32,
    /// Summed link speed of those interfaces, when known (0 otherwise)
    pub system_network_capacity_bytes_ps: f32,
    pub gpu_percent: Option<f32>,
    pub gpu_memory_percent: Option<f32>,
}

impl ColumnTotals {
    pub fn from_readings(readings: &Readings, gpu: &GpuSnapshot) -> Self {
        let mem = &readings.mem_info;
        let mem_total = mem.mem_total.max(1);
        // https://gitlab.com/procps-ng/procps/-/blob/master/library/meminfo.c?ref_type=heads#L736
        let mem_avail = if mem.mem_available > mem.mem_total {
            mem.mem_free
        } else {
            mem.mem_available
        };
        let memory_used = mem_total.saturating_sub(mem_avail);

        let drive_percent = readings
            .disks_info
            .iter()
            .filter(|disk| disk.capacity_bytes > 0)
            .map(|disk| disk.busy_percent)
            .fold(0., f32::max);

        let swap_percent = if mem.swap_total != 0 {
            (100. - (mem.swap_free as f64 * 100.) / (mem.swap_total as f64)) as f32
        } else {
            0.
        };

        let network_bytes_ps = readings
            .running_processes
            .values()
            .map(|p| p.usage_stats.network_usage.round())
            .sum();

        let physical = || {
            readings.network_connections.values().filter(|c| {
                use mc_magpie::magpie_types::network::ConnectionKind as K;
                matches!(
                    K::try_from(c.kind),
                    Ok(K::Wired | K::Wireless | K::Wwan | K::Bluetooth | K::InfiniBand)
                )
            })
        };
        let system_network_bytes_ps = physical()
            .map(|c| c.tx_rate_bytes_ps + c.rx_rate_bytes_ps)
            .sum();
        let system_network_capacity_bytes_ps = physical()
            .filter_map(|c| c.max_speed_bytes_ps)
            .map(|s| s as f32)
            .sum();

        let gpu_percent = gpu.usage.percent;
        let gpu_memory_percent = gpu
            .devices
            .values()
            .try_fold((0u64, 0u64), |(used, total), device| {
                let memory = &device.memory;
                Some((
                    used.checked_add(
                        memory
                            .dedicated_used_bytes?
                            .checked_add(memory.shared_used_bytes?)?,
                    )?,
                    total.checked_add(
                        memory
                            .dedicated_total_bytes?
                            .checked_add(memory.shared_total_bytes?)?,
                    )?,
                ))
            })
            .filter(|(_, total)| *total > 0)
            .map(|(used, total)| used as f64 * 100. / total as f64)
            .map(|value| value as f32);

        Self {
            cpu_percent: readings.cpu.total_usage_percent,
            memory_percent: memory_used as f32 * 100. / mem_total as f32,
            drive_percent,
            swap_percent,
            network_bytes_ps,
            system_network_bytes_ps,
            system_network_capacity_bytes_ps,
            gpu_percent,
            gpu_memory_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_numeric_sort_keeps_unknown_last_in_both_directions() {
        let mut readings = mc_magpie::synthetic_readings(3, 6);
        for gpu in readings.gpus.values_mut() {
            let telemetry = gpu.telemetry.as_mut().unwrap();
            telemetry.observations.retain(|p| p.pid != 100);
            telemetry
                .clients
                .retain(|c| c.owners.iter().all(|p| p.pid != 100));
        }
        let gpu =
            GpuSnapshot::from_readings(&readings, &mut crate::gpu::GpuIndices::default(), 1000);
        let mut tree = Tree::apps(&readings, false);
        tree.apply_gpu(&readings, &gpu, false);
        let header = tree.find_by_key("processes").unwrap();
        for column in [Column::Gpu, Column::GpuDedicatedMemory] {
            for sort_order in [SortOrder::Ascending, SortOrder::Descending] {
                let mut rows = tree.nodes[header].children.clone();
                tree.sort_children(
                    &mut rows,
                    &ViewOptions {
                        sort_column: Some(column),
                        sort_order,
                        ..Default::default()
                    },
                );
                assert_eq!(tree.nodes[*rows.last().unwrap()].pid, 100);
                let values: Vec<_> = rows
                    .iter()
                    .filter_map(|&r| column.numeric_value(&tree.nodes[r]))
                    .collect();
                assert!(
                    values
                        .windows(2)
                        .all(|pair| if sort_order == SortOrder::Ascending {
                            pair[0] <= pair[1]
                        } else {
                            pair[0] >= pair[1]
                        })
                );
            }
        }
    }

    fn process(pid: u32, parent: u32, name: &str, children: &[u32], cpu: f32) -> Process {
        Process {
            name: name.into(),
            cmd: vec![format!("/usr/bin/{name}"), "--flag".into()],
            exe: format!("/usr/bin/{name}"),
            pid,
            parent,
            children: children.to_vec(),
            usage_stats: ProcessUsageStats {
                cpu_usage: cpu,
                memory_usage: pid as u64 * 1000,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn readings() -> Readings {
        let mut r = Readings::new();
        for p in [
            process(1, 0, "systemd", &[10, 20, 30], 0.),
            process(10, 1, "firefox", &[11, 12], 5.),
            process(11, 10, "firefox-child", &[], 2.),
            process(12, 10, "firefox-gpu", &[], 3.),
            process(20, 1, "bash", &[21], 1.),
            process(21, 20, "top", &[], 7.),
            process(30, 1, "sshd", &[], 0.),
        ] {
            r.running_processes.insert(p.pid, p);
        }
        r.running_apps.insert(
            "org.mozilla.firefox".into(),
            App {
                id: "org.mozilla.firefox".into(),
                name: "Firefox".into(),
                command: "firefox".into(),
                pids: vec![10, 11, 12],
            },
        );
        r
    }

    #[test]
    fn primary_processes_skip_children_inside_the_app() {
        let r = readings();
        let app = &r.running_apps["org.mozilla.firefox"];
        assert_eq!(primary_processes(app, &r.running_processes), vec![10]);
    }

    #[test]
    fn apps_tree_has_sections_apps_and_processes() {
        let tree = Tree::apps(&readings(), false);
        let apps = &tree.nodes[tree.roots[0]];
        let procs = &tree.nodes[tree.roots[1]];
        assert_eq!(apps.children.len(), 1);
        assert_eq!(procs.children.len(), 3);

        let firefox = &tree.nodes[apps.children[0]];
        assert_eq!(firefox.kind, RowKind::App);
        // merged usage of the primary process 10 (5 + 2 + 3)
        assert!((firefox.stats.cpu_usage - 10.).abs() < 1e-4);
        assert_eq!(firefox.children.len(), 1);
    }

    #[test]
    fn processes_inherit_the_app_icon() {
        let tree = Tree::apps(&readings(), false);
        let child = tree.find_by_key("processes/p10/p11").unwrap();
        assert_eq!(
            tree.nodes[child].icon,
            RowIcon::App("org.mozilla.firefox".into())
        );
        let bash = tree.find_by_key("processes/p20").unwrap();
        assert_eq!(tree.nodes[bash].icon, RowIcon::Default);
    }

    #[test]
    fn merged_stats_switch() {
        let unmerged = Tree::apps(&readings(), false);
        let merged = Tree::apps(&readings(), true);
        let key = "processes/p20";
        let u = &unmerged.nodes[unmerged.find_by_key(key).unwrap()];
        let m = &merged.nodes[merged.find_by_key(key).unwrap()];
        assert_eq!(u.stats.cpu_usage, 1.);
        assert_eq!(m.stats.cpu_usage, 8.);
    }

    #[test]
    fn flatten_respects_expansion() {
        let tree = Tree::apps(&readings(), false);
        let mut expanded = HashSet::new();
        let rows = tree.flatten(&expanded, &ViewOptions::default());
        // 2 headers + 1 app + 3 top level processes
        assert_eq!(rows.len(), 6);

        expanded.insert("processes/p20".into());
        let rows = tree.flatten(&expanded, &ViewOptions::default());
        assert_eq!(rows.len(), 7);

        let header = &tree.nodes[tree.roots[1]];
        toggle_expanded(&mut expanded, header);
        let rows = tree.flatten(&expanded, &ViewOptions::default());
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn sorting_by_cpu_descending() {
        let tree = Tree::apps(&readings(), false);
        let options = ViewOptions {
            sort_column: Some(Column::Cpu),
            sort_order: SortOrder::Descending,
            ..Default::default()
        };
        let rows = tree.flatten(&HashSet::new(), &options);
        let names = rows
            .iter()
            .map(|r| tree.nodes[r.node].name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["Apps", "Firefox", "Processes", "firefox", "bash", "sshd"]
        );
    }

    #[test]
    fn search_finds_nested_rows() {
        let tree = Tree::apps(&readings(), false);
        let options = ViewOptions {
            search: "top".into(),
            ..Default::default()
        };
        let rows = tree.flatten(&HashSet::new(), &options);
        assert!(rows.iter().any(|r| tree.nodes[r.node].name == "top"));
        assert!(!rows.iter().any(|r| tree.nodes[r.node].name == "sshd"));
    }

    #[test]
    fn services_tree_and_filter() {
        let mut r = readings();
        r.user_services.insert(
            1,
            Service {
                id: 1,
                name: "pipewire.service".into(),
                enabled: true,
                running: true,
                pid: Some(20),
                ..Default::default()
            },
        );
        r.system_services.insert(
            2,
            Service {
                id: 2,
                name: "cups.service".into(),
                enabled: true,
                failed: true,
                ..Default::default()
            },
        );
        let tree = Tree::services(&r, false);
        let all = tree.flatten(&HashSet::new(), &ViewOptions::default());
        assert_eq!(all.len(), 4);

        let failed_only = ViewOptions {
            service_filter: ServiceFilter {
                failed: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let rows = tree.flatten(&HashSet::new(), &failed_only);
        let names = rows
            .iter()
            .map(|r| tree.nodes[r.node].name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["User Services", "System Services", "cups.service"]);
    }

    #[test]
    fn column_order_parsing() {
        let order = parse_column_order("cpu;name;bogus;cpu");
        assert_eq!(order[0], Column::Cpu);
        assert_eq!(order[1], Column::Name);
        assert_eq!(order.len(), Column::ALL.len());
        assert_eq!(parse_column_order(&serialize_column_order(&order)), order);
    }

    #[test]
    fn pretty_names() {
        let mut p = process(5, 1, "x", &[], 0.);
        p.exe = "/usr/lib/wine/wine64-preloader".into();
        p.cmd = vec!["C:\\Games\\game.exe".into()];
        assert_eq!(pretty_process_name(&p), "game.exe");

        p.exe = String::new();
        p.cmd = vec!["kworker/0:1H:".into()];
        assert_eq!(pretty_process_name(&p), "0:1H");
    }

    #[test]
    fn system_network_total_counts_physical_interfaces_only() {
        use mc_magpie::magpie_types::network::{Connection, ConnectionKind};
        let mut r = Readings::new();
        for (id, kind, rate) in [
            ("eth0", ConnectionKind::Wired, 1000.),
            ("wlan0", ConnectionKind::Wireless, 500.),
            ("br-1", ConnectionKind::Bridge, 1000.),
            ("docker0", ConnectionKind::Docker, 1000.),
        ] {
            r.network_connections.insert(
                id.into(),
                Connection {
                    id: id.into(),
                    kind: kind as i32,
                    tx_rate_bytes_ps: rate,
                    rx_rate_bytes_ps: rate,
                    ..Default::default()
                },
            );
        }
        assert_eq!(
            ColumnTotals::from_readings(&r, &GpuSnapshot::default()).system_network_bytes_ps,
            3000.
        );
    }
}
