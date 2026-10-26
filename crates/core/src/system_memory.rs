//! System memory diagnostics: Core RSS, child processes, Linux page cache, and heap trimming.
use crate::{auth::AuthState, identity::Role};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMemoryBreakdown {
    pub pid: u32,
    pub name: String,
    pub rss_bytes: u64,
    pub anon_bytes: u64,
    pub file_bytes: u64,
    pub peak_rss_bytes: u64,
    pub virt_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildProcessMemory {
    pub pid: u32,
    pub name: String,
    pub rss_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostMemoryOverview {
    pub mem_total_bytes: u64,
    pub mem_available_bytes: u64,
    pub mem_free_bytes: u64,
    pub linux_cached_bytes: u64,
    pub linux_buffers_bytes: u64,
    pub linux_sreclaimable_bytes: u64,
    pub linux_reclaimable_cache_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySummaryDiagnosis {
    pub core_rss_bytes: u64,
    pub children_rss_bytes: u64,
    pub total_aether_rss_bytes: u64,
    pub linux_page_cache_bytes: u64,
    pub diagnosis: String,
    pub reclaimable_advice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemoryReport {
    pub core: ProcessMemoryBreakdown,
    pub children: Vec<ChildProcessMemory>,
    pub children_total_rss_bytes: u64,
    pub host: Option<HostMemoryOverview>,
    pub summary: MemorySummaryDiagnosis,
}

/// Request the operating system memory allocator to release unused arena chunks back to the kernel.
#[inline]
pub fn trim_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        let _ = malloc_trim(0);
    }
}

/// Collect a full memory diagnostics report of Core, child processes, and host cache.
pub fn collect_memory_report() -> SystemMemoryReport {
    let core = inspect_current_process();
    let children = inspect_child_processes(core.pid);
    let children_total_rss_bytes: u64 = children.iter().map(|c| c.rss_bytes).sum();
    let host = inspect_host_memory();

    let total_aether_rss_bytes = core.rss_bytes.saturating_add(children_total_rss_bytes);
    let linux_page_cache_bytes = host
        .as_ref()
        .map(|h| h.linux_reclaimable_cache_bytes)
        .unwrap_or(0);

    let (diagnosis, reclaimable_advice) =
        generate_diagnosis(&core, children_total_rss_bytes, host.as_ref());

    let summary = MemorySummaryDiagnosis {
        core_rss_bytes: core.rss_bytes,
        children_rss_bytes: children_total_rss_bytes,
        total_aether_rss_bytes,
        linux_page_cache_bytes,
        diagnosis,
        reclaimable_advice,
    };

    SystemMemoryReport {
        core,
        children,
        children_total_rss_bytes,
        host,
        summary,
    }
}

fn generate_diagnosis(
    core: &ProcessMemoryBreakdown,
    children_rss: u64,
    host: Option<&HostMemoryOverview>,
) -> (String, String) {
    let to_mb = |bytes: u64| (bytes as f64) / (1024.0 * 1024.0);
    let core_mb = to_mb(core.rss_bytes);
    let anon_mb = to_mb(core.anon_bytes);
    let file_mb = to_mb(core.file_bytes);
    let children_mb = to_mb(children_rss);

    if let Some(h) = host {
        let cache_mb = to_mb(h.linux_reclaimable_cache_bytes);
        let total_aether_mb = to_mb(core.rss_bytes + children_rss);

        if cache_mb > total_aether_mb * 2.0 && cache_mb > 100.0 {
            let diagnosis = format!(
                "La mémoire visible (ex: docker stats) est à {:.0}% constituée du cache de pages Linux ({:.1} Mo), généré par le balayage ou la lecture de médias. Aether n'occupe réellement que {:.1} Mo (Core: {:.1} Mo, Enfants: {:.1} Mo).",
                (cache_mb / (cache_mb + total_aether_mb).max(1.0)) * 100.0,
                cache_mb,
                total_aether_mb,
                core_mb,
                children_mb
            );
            let advice = "Le cache de pages Linux est géré par le noyau et sera automatiquement et instantanément libéré dès qu'une application demandera de la mémoire. Il ne s'agit pas d'une fuite de mémoire.".to_string();
            return (diagnosis, advice);
        }
    }

    let diagnosis = format!(
        "Core: {:.1} Mo RSS ({:.1} Mo tas anonyme, {:.1} Mo fichiers/mmap). Processus enfants: {:.1} Mo.",
        core_mb, anon_mb, file_mb, children_mb
    );
    let advice = "Pour réduire la consommation mémoire, le bouton 'Libérer la mémoire' déclenche la purge des caches volatils et la restitution immédiate de la mémoire non allouée à l'OS (malloc_trim).".to_string();
    (diagnosis, advice)
}

fn inspect_current_process() -> ProcessMemoryBreakdown {
    let pid = std::process::id();
    let name = "aether-core".to_string();

    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            let mut rss_kb = 0u64;
            let mut anon_kb = 0u64;
            let mut file_kb = 0u64;
            let mut peak_kb = 0u64;
            let mut virt_kb = 0u64;

            for line in status.lines() {
                let mut parts = line.split_whitespace();
                let key = parts.next().unwrap_or("");
                let val = parts
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                match key {
                    "VmRSS:" => rss_kb = val,
                    "RssAnon:" => anon_kb = val,
                    "RssFile:" => file_kb = val,
                    "VmHWM:" => peak_kb = val,
                    "VmSize:" => virt_kb = val,
                    _ => {}
                }
            }

            return ProcessMemoryBreakdown {
                pid,
                name,
                rss_bytes: rss_kb * 1024,
                anon_bytes: anon_kb * 1024,
                file_bytes: file_kb * 1024,
                peak_rss_bytes: peak_kb * 1024,
                virt_bytes: virt_kb * 1024,
            };
        }
    }

    #[cfg(windows)]
    {
        #[repr(C)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn K32GetProcessMemoryInfo(
                h_process: *mut std::ffi::c_void,
                ppsmem_counters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0 {
                return ProcessMemoryBreakdown {
                    pid,
                    name,
                    rss_bytes: counters.working_set_size as u64,
                    anon_bytes: counters.pagefile_usage as u64,
                    file_bytes: counters
                        .working_set_size
                        .saturating_sub(counters.pagefile_usage)
                        as u64,
                    peak_rss_bytes: counters.peak_working_set_size as u64,
                    virt_bytes: counters.pagefile_usage as u64,
                };
            }
        }
    }

    ProcessMemoryBreakdown {
        pid,
        name,
        rss_bytes: 0,
        anon_bytes: 0,
        file_bytes: 0,
        peak_rss_bytes: 0,
        virt_bytes: 0,
    }
}

fn inspect_child_processes(parent_pid: u32) -> Vec<ChildProcessMemory> {
    #[allow(unused_mut)]
    let mut children = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let proc_dir = match std::fs::read_dir("/proc") {
            Ok(d) => d,
            Err(_) => return children,
        };

        for entry in proc_dir.flatten() {
            let file_name = entry.file_name();
            let pid_str = file_name.to_string_lossy();
            let pid: u32 = match pid_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let stat_path = entry.path().join("stat");
            if let Ok(stat_content) = std::fs::read_to_string(&stat_path) {
                // /proc/[pid]/stat format: pid (comm) state ppid ...
                if let Some(closing_paren) = stat_content.rfind(')') {
                    let after_paren = &stat_content[closing_paren + 1..];
                    let mut fields = after_paren.split_whitespace();
                    let _state = fields.next();
                    let ppid: u32 = fields.next().and_then(|p| p.parse().ok()).unwrap_or(0);

                    if ppid == parent_pid {
                        // Extract comm from inside the parentheses
                        let comm = if let Some(opening_paren) = stat_content.find('(') {
                            stat_content[opening_paren + 1..closing_paren].to_string()
                        } else {
                            format!("process-{pid}")
                        };

                        let mut rss_bytes = 0u64;
                        let status_path = entry.path().join("status");
                        if let Ok(status) = std::fs::read_to_string(&status_path) {
                            for line in status.lines() {
                                if let Some(val) = line
                                    .strip_prefix("VmRSS:")
                                    .and_then(|rest| rest.split_whitespace().next())
                                    .and_then(|v| v.parse::<u64>().ok())
                                {
                                    rss_bytes = val * 1024;
                                    break;
                                }
                            }
                        }

                        children.push(ChildProcessMemory {
                            pid,
                            name: comm,
                            rss_bytes,
                        });
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    let _ = parent_pid;

    children
}

fn inspect_host_memory() -> Option<HostMemoryOverview> {
    #[cfg(target_os = "linux")]
    {
        let content = std::fs::read_to_string("/proc/meminfo").ok()?;
        let mut total = 0u64;
        let mut available = 0u64;
        let mut free = 0u64;
        let mut cached = 0u64;
        let mut buffers = 0u64;
        let mut sreclaimable = 0u64;
        let mut shmem = 0u64;

        for line in content.lines() {
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("");
            let val = parts
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            match key {
                "MemTotal:" => total = val * 1024,
                "MemAvailable:" => available = val * 1024,
                "MemFree:" => free = val * 1024,
                "Cached:" => cached = val * 1024,
                "Buffers:" => buffers = val * 1024,
                "SReclaimable:" => sreclaimable = val * 1024,
                "Shmem:" => shmem = val * 1024,
                _ => {}
            }
        }

        let reclaimable_cache = (cached + buffers + sreclaimable).saturating_sub(shmem);

        Some(HostMemoryOverview {
            mem_total_bytes: total,
            mem_available_bytes: available,
            mem_free_bytes: free,
            linux_cached_bytes: cached,
            linux_buffers_bytes: buffers,
            linux_sreclaimable_bytes: sreclaimable,
            linux_reclaimable_cache_bytes: reclaimable_cache,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/system/memory", get(get_memory_diagnostics))
        .route("/api/v1/admin/system/memory/trim", post(trim_system_memory))
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .with_state(auth)
}

async fn check_admin(auth: &AuthState, headers: &HeaderMap, csrf: bool) -> Result<(), StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if csrf {
        auth.require_csrf(headers, &identity)?;
    }
    Ok(())
}

async fn get_memory_diagnostics(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<SystemMemoryReport>, StatusCode> {
    check_admin(&auth, &headers, false).await?;
    Ok(Json(collect_memory_report()))
}

async fn trim_system_memory(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<SystemMemoryReport>, StatusCode> {
    check_admin(&auth, &headers, true).await?;

    // 1. Invalidate catalogue cache
    crate::inventory::invalidate_catalogue_cache();

    // 2. Instruct SQLite connections to shrink memory
    let _ = crate::database::query("PRAGMA shrink_memory")
        .execute(&auth.store.pool)
        .await;

    // 3. Request OS memory allocator to return unused heap arenas
    trim_memory();

    Ok(Json(collect_memory_report()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_memory_report_is_valid() {
        let report = collect_memory_report();
        assert!(report.core.pid > 0);
        assert_eq!(report.core.name, "aether-core");
        assert!(!report.summary.diagnosis.is_empty());
        assert!(!report.summary.reclaimable_advice.is_empty());
    }

    #[test]
    fn test_trim_memory_does_not_panic() {
        trim_memory();
    }
}
