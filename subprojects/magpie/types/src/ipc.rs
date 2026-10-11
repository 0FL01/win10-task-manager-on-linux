/* src/ipc.rs
 *
 * Copyright 2025 Mission Center Developers
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

use std::num::NonZeroU32;

use crate::about::AboutRequest;
use crate::apps::{AppsIconRequest, AppsRequest};
use crate::battery::BatteryRequest;
use crate::cpu::CpuRequest;
use crate::disks;
use crate::disks::{disks_request, DisksRequest};
use crate::fan::FansRequest;
use crate::gpus::GpusRequest;
use crate::ipc::request::Body;
use crate::memory::{memory_request, MemoryRequest};
use crate::network::ConnectionsRequest;
use crate::processes::{
    processes_request, ContinueProcessesRequest, HangupProcessesRequest, InterruptProcessesRequest,
    KillProcessesRequest, ProcessesRequest, SuspendProcessesRequest, TerminateProcessesRequest,
    User1ProcessesRequest, User2ProcessesRequest,
};
use crate::services::{
    services_request, RequestServiceDisable, RequestServiceEnable, RequestServiceLogs,
    RequestServiceRestart, RequestServiceStart, RequestServiceStop, ServicesRequest,
};
use crate::setup_script::{script_request, ScriptRequest};

include!(concat!(env!("OUT_DIR"), "/magpie.ipc.rs"));

pub fn req_get_about() -> Request {
    Request {
        body: Some(Body::GetAbout(AboutRequest {})),
    }
}

pub fn req_get_apps() -> Request {
    Request {
        body: Some(Body::GetApps(AppsRequest {})),
    }
}

pub fn req_get_app_icons(app_ids: Vec<String>) -> Request {
    Request {
        body: Some(Body::GetAppsIcons(AppsIconRequest { app_ids })),
    }
}

pub fn req_get_batteries() -> Request {
    Request {
        body: Some(Body::GetBattery(BatteryRequest {})),
    }
}

pub fn req_get_cpu() -> Request {
    Request {
        body: Some(Body::GetCpu(CpuRequest {})),
    }
}

pub fn req_get_disks() -> Request {
    Request {
        body: Some(Body::GetDisks(DisksRequest {
            request: Some(disks_request::Request::Disks(Default::default())),
        })),
    }
}

pub fn req_eject_disk(id: String) -> Request {
    Request {
        body: Some(Body::GetDisks(DisksRequest {
            request: Some(disks_request::Request::Eject(disks::EjectRequest { id })),
        })),
    }
}

pub fn req_get_smart_data(id: String) -> Request {
    Request {
        body: Some(Body::GetDisks(DisksRequest {
            request: Some(disks_request::Request::Smart(disks::SmartRequest { id })),
        })),
    }
}

pub fn req_get_fans() -> Request {
    Request {
        body: Some(Body::GetFans(FansRequest {})),
    }
}

pub fn req_get_gpus() -> Request {
    Request {
        body: Some(Body::GetGpus(GpusRequest {})),
    }
}

pub fn req_get_memory(kind: memory_request::Kind) -> Request {
    Request {
        body: Some(Body::GetMemory(MemoryRequest { kind: kind as i32 })),
    }
}

pub fn req_get_connections() -> Request {
    Request {
        body: Some(Body::GetConnections(ConnectionsRequest {})),
    }
}

pub fn req_get_processes() -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::ProcessMap(Default::default())),
        })),
    }
}

pub fn req_terminate_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Terminate(
                TerminateProcessesRequest { pids },
            )),
        })),
    }
}

pub fn req_kill_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Kill(KillProcessesRequest {
                pids,
            })),
        })),
    }
}

pub fn req_interrupt_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Interrupt(
                InterruptProcessesRequest { pids },
            )),
        })),
    }
}

pub fn req_signal_user_one_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::User1(User1ProcessesRequest {
                pids,
            })),
        })),
    }
}

pub fn req_signal_user_two_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::User2(User2ProcessesRequest {
                pids,
            })),
        })),
    }
}

pub fn req_hangup_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Hangup(HangupProcessesRequest {
                pids,
            })),
        })),
    }
}

pub fn req_continue_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Continue(
                ContinueProcessesRequest { pids },
            )),
        })),
    }
}

pub fn req_suspend_processes(pids: Vec<u32>) -> Request {
    Request {
        body: Some(Body::GetProcesses(ProcessesRequest {
            request: Some(processes_request::Request::Suspend(
                SuspendProcessesRequest { pids },
            )),
        })),
    }
}

pub fn req_get_system_services() -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::SystemServiceList(
                Default::default(),
            )),
        })),
    }
}

pub fn req_get_user_services() -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::UserServiceList(
                Default::default(),
            )),
        })),
    }
}

pub fn req_get_logs(service_id: u64, pid: Option<NonZeroU32>) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceLogs(RequestServiceLogs {
                service_id,
                pid: pid.map(|pid| pid.get()),
            })),
        })),
    }
}

pub fn req_start_service(service_id: u64) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceStart(
                RequestServiceStart { service_id },
            )),
        })),
    }
}

pub fn req_stop_service(service_id: u64) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceStop(RequestServiceStop {
                service_id,
            })),
        })),
    }
}

pub fn req_restart_service(service_id: u64) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceRestart(
                RequestServiceRestart { service_id },
            )),
        })),
    }
}

pub fn req_enable_service(service_id: u64) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceEnable(
                RequestServiceEnable { service_id },
            )),
        })),
    }
}

pub fn req_disable_service(service_id: u64) -> Request {
    Request {
        body: Some(Body::GetServices(ServicesRequest {
            request: Some(services_request::Request::ServiceDisable(
                RequestServiceDisable { service_id },
            )),
        })),
    }
}

pub fn req_script_name() -> Request {
    Request {
        body: Some(Body::GetScript(ScriptRequest {
            request: Some(script_request::Request::GetName(Default::default())),
        })),
    }
}

pub fn req_script_run() -> Request {
    Request {
        body: Some(Body::GetScript(ScriptRequest {
            request: Some(script_request::Request::Run(Default::default())),
        })),
    }
}

pub fn req_script_run_revert() -> Request {
    Request {
        body: Some(Body::GetScript(ScriptRequest {
            request: Some(script_request::Request::RunRevert(Default::default())),
        })),
    }
}

pub fn req_script_open() -> Request {
    Request {
        body: Some(Body::GetScript(ScriptRequest {
            request: Some(script_request::Request::Open(Default::default())),
        })),
    }
}
