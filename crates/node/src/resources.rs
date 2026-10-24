//! Low-frequency admission telemetry. Missing OS counters remain unknown rather than zero.
//! CPU describes aggregate host CPU (the current processor group on Windows >64 CPUs).
use media_contracts::protocol::{ResourcePressure, unix_now};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Previous {
    sampled: Option<Instant>,
    cpu: Option<(u64, u64)>,
    network: Option<u64>,
    result: ResourcePressure,
}
pub struct Sampler {
    scratch: PathBuf,
    previous: Arc<Mutex<Previous>>,
}
impl Sampler {
    pub fn new(scratch: PathBuf) -> Self {
        Self {
            scratch,
            previous: Arc::new(Mutex::new(Previous::default())),
        }
    }
    /// File reads and OS calls stay outside Tokio's async worker threads. Concurrent
    /// heartbeats coalesce and reuse a cached sample for at least one second.
    pub async fn sample(&self) -> ResourcePressure {
        let previous = self.previous.clone();
        let scratch = self.scratch.clone();
        tokio::task::spawn_blocking(move || {
            let mut previous = previous.lock().unwrap_or_else(|p| p.into_inner());
            let clock = Instant::now();
            if previous
                .sampled
                .is_some_and(|last| clock.duration_since(last) < Duration::from_secs(1))
            {
                return previous.result.clone();
            }
            let raw = read_os(&scratch);
            let cpu =
                raw.cpu
                    .zip(previous.cpu)
                    .and_then(|((total, idle), (old_total, old_idle))| {
                        cpu_percent(total, idle, old_total, old_idle)
                    });
            let network = raw
                .network
                .zip(previous.network)
                .zip(previous.sampled)
                .and_then(|((current, old), last)| {
                    let nanos = clock.duration_since(last).as_nanos();
                    if nanos == 0 {
                        return None;
                    }
                    let delta = current.checked_sub(old)?;
                    Some(
                        (u128::from(delta) * 1_000_000_000 / nanos).min(u128::from(u64::MAX))
                            as u64,
                    )
                });
            let result = ResourcePressure {
                sampled_at: unix_now(),
                cpu_usage_percent: cpu,
                memory_available_bytes: raw.available,
                memory_total_bytes: raw.total,
                scratch_available_bytes: raw.disk,
                io_pressure_percent: raw.io,
                network_bytes_per_second: network,
            };
            previous.cpu = raw.cpu;
            previous.network = raw.network;
            previous.sampled = Some(clock);
            previous.result = result.clone();
            result
        })
        .await
        .unwrap_or_default()
    }
}
fn cpu_percent(total: u64, idle: u64, old_total: u64, old_idle: u64) -> Option<u8> {
    let total = total.checked_sub(old_total)?;
    let idle = idle.checked_sub(old_idle)?;
    if total == 0 || idle > total {
        return None;
    }
    Some(((u128::from(total - idle) * 100) / u128::from(total)) as u8)
}
#[derive(Default)]
struct Raw {
    cpu: Option<(u64, u64)>,
    network: Option<u64>,
    available: Option<u64>,
    total: Option<u64>,
    disk: Option<u64>,
    io: Option<u8>,
}

#[cfg(target_os = "linux")]
fn bounded(path: &str) -> Option<String> {
    use std::io::Read;
    let mut result = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_string(&mut result)
        .ok()?;
    Some(result)
}
#[cfg(target_os = "linux")]
fn memory_field(text: &str, name: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != name {
            return None;
        }
        fields.next()?.parse::<u64>().ok()?.checked_mul(1024)
    })
}
#[cfg(target_os = "linux")]
fn read_os(scratch: &Path) -> Raw {
    let cpu = bounded("/proc/stat").and_then(|text| {
        let mut parts = text.lines().next()?.split_whitespace();
        if parts.next()? != "cpu" {
            return None;
        }
        // guest/guest_nice are already included in user/nice and must not be counted twice.
        let parts: Vec<u64> = parts
            .take(8)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if parts.len() < 4 {
            return None;
        }
        let total = parts.iter().try_fold(0u64, |acc, n| acc.checked_add(*n))?;
        Some((
            total,
            parts[3].checked_add(parts.get(4).copied().unwrap_or(0))?,
        ))
    });
    let mut total = None;
    let mut available = None;
    if let Some(mem) = bounded("/proc/meminfo") {
        total = memory_field(&mem, "MemTotal:");
        available = memory_field(&mem, "MemAvailable:");
    }
    // Respect unified cgroup RAM limits when the common container mount exposes them.
    if let (Some(max), Some(current)) = (
        bounded("/sys/fs/cgroup/memory.max").and_then(|s| s.trim().parse::<u64>().ok()),
        bounded("/sys/fs/cgroup/memory.current").and_then(|s| s.trim().parse::<u64>().ok()),
    ) {
        total = Some(total.map_or(max, |tot| tot.min(max)));
        let avail = max.saturating_sub(current);
        available = Some(available.map_or(avail, |host| host.min(avail)));
    }
    let network = bounded("/proc/net/dev").and_then(|text| {
        let mut total = 0u64;
        let mut seen = false;
        for line in text.lines().skip(2) {
            let (name, counts) = line.split_once(':')?;
            if name.trim() == "lo" {
                continue;
            }
            let counts: Vec<&str> = counts.split_whitespace().take(16).collect();
            let received = counts.first()?.parse::<u64>().ok()?;
            let sent = counts.get(8)?.parse::<u64>().ok()?;
            total = total.checked_add(received)?.checked_add(sent)?;
            seen = true;
        }
        seen.then_some(total)
    });
    let io = bounded("/proc/pressure/io").and_then(|text| {
        let line = text.lines().find(|line| line.starts_with("some "))?;
        let avg = line
            .split_whitespace()
            .find_map(|part| part.strip_prefix("avg10="))?
            .parse::<f64>()
            .ok()?;
        avg.is_finite()
            .then_some(avg.clamp(0.0, 100.0).round() as u8)
    });
    let disk = rustix::fs::statvfs(scratch)
        .ok()
        .and_then(|stat| stat.f_bavail.checked_mul(stat.f_frsize));
    Raw {
        cpu,
        network,
        available,
        total,
        disk,
        io,
    }
}

#[cfg(windows)]
fn read_os(scratch: &Path) -> Raw {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }
    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatus {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        page_total: u64,
        page_available: u64,
        virtual_total: u64,
        virtual_available: u64,
        extended_available: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetSystemTimes"]
        fn get_system_times(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime)
        -> i32;
        #[link_name = "GlobalMemoryStatusEx"]
        fn memory_status(status: *mut MemoryStatus) -> i32;
        #[link_name = "GetDiskFreeSpaceExW"]
        fn disk_free(path: *const u16, available: *mut u64, total: *mut u64, free: *mut u64)
        -> i32;
    }
    let mut raw = Raw::default();
    let (mut idle, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    // SAFETY: all output pointers reference initialized, correctly laid out writable
    // objects for the entire synchronous Windows API call. No pointers are retained.
    if unsafe { get_system_times(&mut idle, &mut kernel, &mut user) } != 0 {
        raw.cpu = kernel
            .ticks()
            .checked_add(user.ticks())
            .map(|total| (total, idle.ticks()));
    }
    let mut memory = MemoryStatus {
        length: std::mem::size_of::<MemoryStatus>() as u32,
        ..Default::default()
    };
    // SAFETY: length matches the repr(C) MEMORYSTATUSEX layout and memory is writable.
    if unsafe { memory_status(&mut memory) } != 0 {
        raw.total = Some(memory.total);
        raw.available = Some(memory.available);
    }
    let mut path: Vec<u16> = scratch.as_os_str().encode_wide().collect();
    if !path.contains(&0) {
        path.push(0);
        let (mut available, mut total, mut free) = (0, 0, 0);
        // SAFETY: path is NUL-terminated UTF-16, all outputs are valid u64 pointers.
        if unsafe { disk_free(path.as_ptr(), &mut available, &mut total, &mut free) } != 0 {
            raw.disk = Some(available);
        }
    }
    raw
}
#[cfg(not(any(target_os = "linux", windows)))]
fn read_os(_scratch: &Path) -> Raw {
    Raw::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_deltas_reject_resets_and_idle_overflow() {
        assert_eq!(cpu_percent(200, 75, 100, 25), Some(50));
        assert_eq!(cpu_percent(100, 20, 100, 10), None);
        assert_eq!(cpu_percent(50, 10, 100, 25), None);
        assert_eq!(cpu_percent(200, 200, 100, 0), None);
    }
    #[tokio::test]
    async fn sampler_coalesces_and_reports_real_available_counters() {
        let temp = tempfile::tempdir().unwrap();
        let sampler = Sampler::new(temp.path().to_owned());
        let first = sampler.sample().await;
        let second = sampler.sample().await;
        assert_eq!(first, second);
        assert!(first.cpu_usage_percent.is_none());
        #[cfg(any(target_os = "linux", windows))]
        {
            assert!(first.memory_total_bytes.is_some_and(|n| n > 0));
            assert!(first.memory_available_bytes.is_some());
            assert!(first.scratch_available_bytes.is_some());
        }
    }
}
