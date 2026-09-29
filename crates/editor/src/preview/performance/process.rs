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

#[cfg(not(target_os = "macos"))]
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
