//! Local OS statistics for the owned Engine child; no IPC or render-loop wakeup.
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProcessUsage {
    pub cpu_time: Duration,
    pub resident_bytes: u64,
}

#[cfg(target_os = "macos")]
// Keep the existing locked libc ABI bindings; no additional Mach dependency.
#[allow(deprecated)]
pub(in crate::preview) fn read(pid: u32) -> Option<ProcessUsage> {
    use std::mem::MaybeUninit;
    use std::sync::OnceLock;

    static TIMEBASE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    let &(numer, denom) = TIMEBASE
        .get_or_init(|| {
            let mut info = MaybeUninit::<libc::mach_timebase_info>::zeroed();
            // SAFETY: the OS writes this exact libc ABI struct on success.
            if unsafe { libc::mach_timebase_info(info.as_mut_ptr()) } != 0 {
                return None;
            }
            // SAFETY: the successful call initialized info.
            let info = unsafe { info.assume_init() };
            (info.numer > 0 && info.denom > 0).then_some((info.numer, info.denom))
        })
        .as_ref()?;
    let pid = i32::try_from(pid).ok()?;
    let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: RUSAGE_INFO_V2 writes rusage_info_v2 into this suitably aligned
    // allocation. libproc's rusage_info_t* is the C API's untyped buffer, not
    // an extra indirection. Only use the initialized result on return 0.
    // Apple contract: xnu/libsyscall/wrappers/libproc/libproc.h.
    if unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) } != 0 {
        return None;
    }
    // SAFETY: the successful V2 query initialized the complete V2 struct.
    let usage = unsafe { usage.assume_init() };
    if usage.ri_proc_exit_abstime != 0 {
        return None;
    }
    // XNU fill_task_rusage copies task_power_info's Mach absolute-time ticks,
    // not nanoseconds. Convert with the host timebase (important on arm64).
    // See apple-oss-distributions/xnu: osfmk/kern/{bsd_kern,task}.c.
    let ticks = usage.ri_user_time.checked_add(usage.ri_system_time)?;
    Some(ProcessUsage {
        cpu_time: mach_duration(ticks, numer, denom)?,
        resident_bytes: usage.ri_resident_size,
    })
}

#[cfg(target_os = "macos")]
fn mach_duration(ticks: u64, numer: u32, denom: u32) -> Option<Duration> {
    let nanos = (u128::from(ticks) * u128::from(numer)).checked_div(u128::from(denom))?;
    Some(Duration::from_nanos(u64::try_from(nanos).ok()?))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(in crate::preview) fn read(_pid: u32) -> Option<ProcessUsage> {
    // No reliable implementation is enabled on these platforms yet.
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn mach_ticks_use_timebase_without_overflow_or_division_by_zero() {
        assert_eq!(
            mach_duration(24_000_000, 125, 3),
            Some(Duration::from_secs(1))
        );
        assert_eq!(mach_duration(u64::MAX, u32::MAX, 1), None);
        assert_eq!(mach_duration(1, 1, 0), None);
    }
}

#[cfg(target_os = "linux")]
pub(in crate::preview) fn read(pid: u32) -> Option<ProcessUsage> {
    use std::io::Read;
    use std::sync::OnceLock;
    static UNITS: OnceLock<Option<(u64, u64)>> = OnceLock::new();
    let &(ticks, page_bytes) = UNITS
        .get_or_init(|| {
            // SAFETY: sysconf has no pointer arguments; these are read-only queries.
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            // SAFETY: same contract as above.
            let pages = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            Some((u64::try_from(ticks).ok()?, u64::try_from(pages).ok()?))
                .filter(|(ticks, pages)| *ticks > 0 && *pages > 0)
        })
        .as_ref()?;
    let mut stat = String::new();
    std::fs::File::open(format!("/proc/{pid}/stat"))
        .ok()?
        .take(8193)
        .read_to_string(&mut stat)
        .ok()?;
    (stat.len() <= 8192).then_some(())?;
    linux_usage(&stat, ticks, page_bytes)
}

#[cfg(any(target_os = "linux", test))]
fn linux_usage(stat: &str, ticks_per_second: u64, page_bytes: u64) -> Option<ProcessUsage> {
    // comm (field 2) may contain spaces and parentheses; its final ')' ends it.
    let fields: Vec<_> = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .take(22)
        .collect();
    if matches!(*fields.first()?, "Z" | "X" | "x") || page_bytes == 0 {
        return None;
    }
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    let resident: u64 = fields.get(21)?.parse().ok()?;
    let ticks = user.checked_add(system)?;
    let nanos = (u128::from(ticks) * 1_000_000_000).checked_div(u128::from(ticks_per_second))?;
    Some(ProcessUsage {
        cpu_time: Duration::from_nanos(u64::try_from(nanos).ok()?),
        resident_bytes: resident.checked_mul(page_bytes)?,
    })
}

#[cfg(windows)]
pub(in crate::preview) fn read(pid: u32) -> Option<ProcessUsage> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{
                GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    // SAFETY: no inherited handle, only query access to the owned child.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let result = (|| {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let mut memory = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let mut status = 0;
        // SAFETY: valid live handle and writable buffers with exact ABI sizes.
        if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
            || unsafe { K32GetProcessMemoryInfo(handle, &mut memory, memory.cb) } == 0
            || unsafe { GetExitCodeProcess(handle, &mut status) } == 0
            || status != 259
        // STILL_ACTIVE
        {
            return None;
        }
        let ticks =
            |time: FILETIME| u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime);
        let nanos = u128::from(ticks(kernel).checked_add(ticks(user))?) * 100;
        Some(ProcessUsage {
            cpu_time: Duration::from_nanos(u64::try_from(nanos).ok()?),
            resident_bytes: memory.WorkingSetSize as u64,
        })
    })();
    // SAFETY: release exactly the owned handle, including failed queries.
    unsafe { CloseHandle(handle) };
    result
}

#[cfg(test)]
mod portable_tests {
    use super::*;
    #[test]
    fn linux_stat_handles_unusual_names_units_and_invalid_counters() {
        let mut fields = vec!["0"; 22];
        fields[0] = "R";
        fields[11] = "125";
        fields[12] = "25";
        fields[21] = "3";
        let stat = format!("42 (a tricky ) name) {}", fields.join(" "));
        let usage = linux_usage(&stat, 100, 4096).unwrap();
        assert_eq!(usage.cpu_time, Duration::from_millis(1500));
        assert_eq!(usage.resident_bytes, 12288);
        assert!(linux_usage(&stat, 0, 4096).is_none());
        assert!(linux_usage(&stat, 100, u64::MAX).is_none());
        fields[0] = "Z";
        assert!(linux_usage(&format!("42 (done) {}", fields.join(" ")), 100, 4096).is_none());
        fields[0] = "R";
        fields[21] = "-1";
        assert!(linux_usage(&format!("42 (bad) {}", fields.join(" ")), 100, 4096).is_none());
        assert!(linux_usage("42 (truncated) R", 100, 4096).is_none());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn system_query_reports_current_process_and_rejects_missing_pid() {
        let usage = read(std::process::id()).unwrap();
        assert!(usage.resident_bytes > 0);
        assert!(read(u32::MAX).is_none());
    }
}
