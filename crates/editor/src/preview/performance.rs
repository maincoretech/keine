//! Workspace-owned, memory-only history. Closing the tool view cannot stop sampling.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::PreviewLifecycle;
pub(crate) mod process;
use process::ProcessUsage;

pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
pub const HISTORY_WINDOW: Duration = Duration::from_secs(30);
const SAMPLE_CAPACITY: usize = 61;
const LIFECYCLE_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct Sample {
    pub at: Instant,
    pub process: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub resident_bytes: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct LifecycleSample {
    pub at: Instant,
    pub state: PreviewLifecycle,
}

#[derive(Clone, Debug, Default)]
pub struct PerformanceSnapshot {
    pub samples: VecDeque<Sample>,
    pub lifecycle: VecDeque<LifecycleSample>,
    pub pid: Option<u32>,
    pub process: Option<u64>,
    pub observed_peak: Option<u64>,
    pub revision: u64,
}

pub(super) struct PerformanceState {
    snapshot: PerformanceSnapshot,
    baseline: Option<(Instant, Duration)>,
    next_process: u64,
}

impl PerformanceState {
    pub(super) fn new(at: Instant) -> Self {
        let mut state = Self {
            snapshot: PerformanceSnapshot::default(),
            baseline: None,
            next_process: 0,
        };
        state.lifecycle(at, &PreviewLifecycle::Off);
        state
    }

    pub(super) fn snapshot(&self) -> PerformanceSnapshot {
        self.snapshot.clone()
    }

    pub(super) fn begin_process(&mut self, pid: u32) {
        // A new lifetime is distinct even if the OS reuses the same PID.
        self.next_process = self.next_process.wrapping_add(1).max(1);
        self.snapshot.process = Some(self.next_process);
        self.snapshot.pid = Some(pid);
        self.snapshot.observed_peak = None;
        self.baseline = None;
        self.changed();
    }

    pub(super) fn end_process(&mut self) {
        self.snapshot.process = None;
        self.snapshot.pid = None;
        self.snapshot.observed_peak = None;
        self.baseline = None;
        self.changed();
    }

    pub(super) fn sample(&mut self, at: Instant, usage: Option<ProcessUsage>) {
        let usage = usage.filter(|_| self.snapshot.process.is_some());
        let cpu_percent = usage.and_then(|usage| {
            let (previous_at, previous_cpu) = self.baseline?;
            let elapsed = at.checked_duration_since(previous_at)?;
            if elapsed.is_zero() {
                return None;
            }
            // One logical core = 100%; no normalization or clamp to 100%.
            let delta = usage.cpu_time.checked_sub(previous_cpu)?;
            Some(delta.as_secs_f64() / elapsed.as_secs_f64() * 100.)
        });
        // Unavailable queries break the CPU baseline, rather than fabricate zero.
        self.baseline = usage.map(|usage| (at, usage.cpu_time));
        let resident_bytes = usage.map(|usage| usage.resident_bytes);
        if let Some(bytes) = resident_bytes {
            self.snapshot.observed_peak = Some(self.snapshot.observed_peak.unwrap_or(0).max(bytes));
        }
        self.snapshot.samples.push_back(Sample {
            at,
            process: self.snapshot.process,
            cpu_percent,
            resident_bytes,
        });
        while self.snapshot.samples.len() > SAMPLE_CAPACITY
            || self
                .snapshot
                .samples
                .front()
                .is_some_and(|sample| at.saturating_duration_since(sample.at) > HISTORY_WINDOW)
        {
            self.snapshot.samples.pop_front();
        }
        self.prune_lifecycle(at);
        self.changed();
    }

    pub(super) fn lifecycle(&mut self, at: Instant, state: &PreviewLifecycle) {
        if self
            .snapshot
            .lifecycle
            .back()
            .is_some_and(|last| last.state == *state)
        {
            return;
        }
        self.snapshot.lifecycle.push_back(LifecycleSample {
            at,
            state: state.clone(),
        });
        self.prune_lifecycle(at);
        self.changed();
    }

    fn prune_lifecycle(&mut self, at: Instant) {
        // Keep one anchor preceding the window so an old Off/Running state still
        // describes the left edge. Rapid transitions are also strictly bounded.
        while self.snapshot.lifecycle.len() > LIFECYCLE_CAPACITY
            || self
                .snapshot
                .lifecycle
                .get(1)
                .is_some_and(|sample| at.saturating_duration_since(sample.at) > HISTORY_WINDOW)
        {
            self.snapshot.lifecycle.pop_front();
        }
    }

    fn changed(&mut self) {
        self.snapshot.revision = self.snapshot.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(seconds: f64, bytes: u64) -> Option<ProcessUsage> {
        Some(ProcessUsage {
            cpu_time: Duration::from_secs_f64(seconds),
            resident_bytes: bytes,
        })
    }

    #[test]
    fn actual_elapsed_time_allows_multiple_cores_and_counter_reset() {
        let at = Instant::now();
        let mut state = PerformanceState::new(at);
        state.begin_process(1);
        state.sample(at, usage(5., 100));
        assert_eq!(state.snapshot.samples.back().unwrap().cpu_percent, None);
        state.sample(at + Duration::from_millis(800), usage(7., 120));
        assert_eq!(
            state.snapshot.samples.back().unwrap().cpu_percent,
            Some(250.)
        );
        state.sample(at + Duration::from_secs(1), usage(1., 80));
        assert_eq!(state.snapshot.samples.back().unwrap().cpu_percent, None);
        state.sample(at + Duration::from_secs(2), usage(1.5, 80));
        assert_eq!(
            state.snapshot.samples.back().unwrap().cpu_percent,
            Some(50.)
        );
    }

    #[test]
    fn history_and_transitions_are_bounded_and_expire() {
        let at = Instant::now();
        let mut state = PerformanceState::new(at);
        for i in 0..200 {
            let now = at + Duration::from_millis(i * 500);
            state.sample(now, None);
            state.lifecycle(
                now,
                if i % 2 == 0 {
                    &PreviewLifecycle::Running
                } else {
                    &PreviewLifecycle::Off
                },
            );
        }
        assert_eq!(state.snapshot.samples.len(), SAMPLE_CAPACITY);
        assert!(state.snapshot.lifecycle.len() <= LIFECYCLE_CAPACITY);
        state.sample(at + Duration::from_secs(200), None);
        assert_eq!(state.snapshot.samples.len(), 1);
        assert_eq!(state.snapshot.lifecycle.len(), 1);
        for i in 0..100 {
            state.lifecycle(
                at + Duration::from_secs(201) + Duration::from_millis(i),
                if i % 2 == 0 {
                    &PreviewLifecycle::Running
                } else {
                    &PreviewLifecycle::Off
                },
            );
        }
        assert_eq!(state.snapshot.lifecycle.len(), LIFECYCLE_CAPACITY);
    }

    #[test]
    fn peak_and_cpu_baseline_reset_even_when_pid_is_reused() {
        let at = Instant::now();
        let mut state = PerformanceState::new(at);
        state.begin_process(42);
        state.sample(at, usage(1., 500));
        state.sample(at + SAMPLE_INTERVAL, usage(1.2, 100));
        let previous = state.snapshot.process;
        assert_eq!(state.snapshot.observed_peak, Some(500));
        state.end_process();
        state.begin_process(42);
        assert_ne!(state.snapshot.process, previous);
        assert_eq!(state.snapshot.observed_peak, None);
        state.sample(at + Duration::from_secs(1), usage(100., 50));
        assert_eq!(state.snapshot.observed_peak, Some(50));
        assert_eq!(state.snapshot.samples.back().unwrap().cpu_percent, None);
        assert_eq!(state.snapshot.samples.len(), 3); // history survives restart
    }

    #[test]
    fn unavailable_queries_leave_gaps_and_preserve_only_observed_peak() {
        let at = Instant::now();
        let mut state = PerformanceState::new(at);
        state.begin_process(42);
        state.sample(at, usage(1., 500));
        state.sample(at + SAMPLE_INTERVAL, None);
        let sample = state.snapshot.samples.back().unwrap();
        assert_eq!(sample.cpu_percent, None);
        assert_eq!(sample.resident_bytes, None);
        assert_eq!(state.snapshot.observed_peak, Some(500));
        state.sample(at + Duration::from_secs(1), usage(10., 300));
        assert_eq!(state.snapshot.samples.back().unwrap().cpu_percent, None);
        state.end_process();
        state.sample(at + Duration::from_secs(2), usage(11., 600));
        assert_eq!(state.snapshot.observed_peak, None);
        assert_eq!(state.snapshot.samples.back().unwrap().resident_bytes, None);
    }
}
