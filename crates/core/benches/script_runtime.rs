//! Runtime stepping throughput for non-blocking actions.
//!
//! Baseline before expression/interpolation precompilation. The program is a
//! single scene of `ACTION_COUNT` comments; `step()` batches at most
//! `MAX_FORWARD_ACTIONS` per call, so the loop continues on `ExecutionLimit`.

use std::sync::OnceLock;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use keine_core::{Action, Program, SayOptions, State, StepResult, Value, step};

const ACTION_COUNT: usize = 100_000;
const DIALOGUE_TURNS: usize = 1_000;

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

fn mixed_program() -> &'static Program {
    static PROGRAM: OnceLock<Program> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        let mut actions = Vec::with_capacity(DIALOGUE_TURNS * 3);
        for _ in 0..DIALOGUE_TURNS {
            actions.push(Action::Set {
                name: "counter".to_string(),
                expression: "counter + 1".to_string(),
                global: false,
            });
            actions.push(Action::Flow {
                action: Box::new(Action::Comment),
                when: Some("enabled && counter >= 0".to_string()),
                next: true,
            });
            actions.push(Action::Say {
                speaker: "Narrator {counter}".to_string(),
                text: "Line {counter}: value {counter + 1}".to_string(),
                options: SayOptions::default(),
            });
        }
        Program::from_scenes([("bench".to_string(), actions)])
    })
}

fn run_mixed_dialogue(state: &mut State) -> usize {
    let mut turns = 0;
    loop {
        match step::step(state) {
            StepResult::AwaitClick => {
                turns += 1;
                step::advance(state);
            }
            StepResult::EndOfScene => break,
            StepResult::ExecutionLimit => continue,
            result => panic!("mixed dialogue program yielded unexpectedly: {result:?}"),
        }
    }
    turns
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

    let mut group = c.benchmark_group("script_runtime_mixed");
    group.throughput(Throughput::Elements(DIALOGUE_TURNS as u64));
    group.bench_function(BenchmarkId::new("dialogue_turns", DIALOGUE_TURNS), |b| {
        b.iter(|| {
            let mut state = State::new();
            state.current_scene = "bench".to_string();
            state.vars.insert("counter".to_string(), Value::Int(0));
            state.vars.insert("enabled".to_string(), Value::Bool(true));
            state.install_program(mixed_program().clone());
            black_box(run_mixed_dialogue(&mut state))
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
