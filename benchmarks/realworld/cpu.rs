//! Benchmark-only CPU accounting, including waited-for subprocesses.
#[cfg(unix)]
pub fn seconds() -> Option<f64> {
    let mut total = 0.0;
    for who in [libc::RUSAGE_SELF, libc::RUSAGE_CHILDREN] {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // getrusage initializes the entire value on success.
        if unsafe { libc::getrusage(who, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        let usage = unsafe { usage.assume_init() };
        total += usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1e6;
        total += usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1e6;
    }
    Some(total)
}

#[cfg(not(unix))]
pub fn seconds() -> Option<f64> {
    None
}

pub fn elapsed(start: Option<f64>) -> Option<f64> {
    seconds()
        .zip(start)
        .map(|(end, start)| (end - start).max(0.0))
}
