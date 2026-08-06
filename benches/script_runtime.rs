//! Runtime stepping throughput for non-blocking actions.
//!
//! Baseline before expression/interpolation precompilation. The program is a
//! single scene of `ACTION_COUNT` comments; `step()` batches at most
//! `MAX_FORWARD_ACTIONS` per call, so the loop continues on `ExecutionLimit`.

use std::sync::OnceLock;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use keine_core::{Action, Program, State, StepResult, step};

const ACTION_COUNT: usize = 100_000;

fn program() -> &'static Program {
    static PROGRAM: OnceLock<Program> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        Program::from_scenes([(
            "bench".to_string(),
            (0..ACTION_COUNT).map(|_| Action::Comment).collect(),
        )])
    })
}

fn run_through(state: &mut State) -> usize {
    loop {
        match step::step(state) {
            StepResult::EndOfScene => break,
            StepResult::ExecutionLimit => continue,
            result => panic!("comment-only program must not yield: {result:?}"),
        }
    }
    state.cursor
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("script_runtime");
    group.throughput(Throughput::Elements(ACTION_COUNT as u64));
    group.bench_function(BenchmarkId::new("step", ACTION_COUNT), |b| {
        b.iter(|| {
            let mut state = State::new();
            state.current_scene = "bench".to_string();
            state.install_program(program().clone());
            black_box(run_through(&mut state))
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
