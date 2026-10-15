//! Benchmark orchestration, separate from normal project bootstrap.
mod report;
mod workloads;

pub(crate) use report::{
    RUNTIME_BENCHMARK_CHILD_ENV, STARTUP_BENCHMARK_CHILD_ENV, run_benchmark_report,
    run_startup_suite,
};
#[cfg(feature = "publisher")]
pub(crate) use workloads::android_benchmark_plan;
pub(crate) use workloads::{benchmark_timelines, resolve_benchmark_target};
