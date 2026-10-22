//! Runtime stepping throughput for non-blocking actions.
//!
//! Baseline before expression/interpolation precompilation. The program is a
//! single scene of `ACTION_COUNT` comments; `step()` batches at most
//! `MAX_FORWARD_ACTIONS` per call, so the loop continues on `ExecutionLimit`.

use std::sync::OnceLock;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use keine_core::{Action, Program, SayOptions, State, StepResult, Value, step};
use std::hint::black_box;

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
            StepResult::RuntimeError(error) => panic!("runtime error: {error:?}"),
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
            StepResult::RuntimeError(error) => panic!("runtime error: {error:?}"),
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
    group.bench_function(BenchmarkId::new("step_preinstalled", ACTION_COUNT), |b| {
        b.iter_batched_ref(
            || {
                let mut state = State::new();
                state.current_scene = "bench".to_string();
                state.install_program(program().clone());
                state
            },
            |state| black_box(run_through(state)),
            BatchSize::LargeInput,
        );
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
    group.bench_function(
        BenchmarkId::new("dialogue_preinstalled", DIALOGUE_TURNS),
        |b| {
            b.iter_batched_ref(
                || {
                    let mut state = State::new();
                    state.current_scene = "bench".to_string();
                    state.vars.insert("counter".to_string(), Value::Int(0));
                    state.vars.insert("enabled".to_string(), Value::Bool(true));
                    state.install_program(mixed_program().clone());
                    state
                },
                |state| black_box(run_mixed_dialogue(state)),
                BatchSize::LargeInput,
            );
        },
    );
    group.finish();
}

fn bench_markup(c: &mut Criterion) {
    let mut group = c.benchmark_group("dialogue_markup");
    for size in [4_096, 16_384, 65_536] {
        for (name, source) in [
            ("unclosed_brackets", "[".repeat(size)),
            ("unclosed_styles", "[a](".repeat(size / 4)),
            (
                "unclosed_arguments",
                format!("{}]({}", "[".repeat(size), "x".repeat(size)),
            ),
            (
                "trailing_whitespace",
                format!("{}{}]", "[".repeat(size), " ".repeat(size)),
            ),
            ("plain", "普通对白 text ".repeat(size / 20)),
            (
                "styled_waits",
                "[字](color=#fff)[wait=100]后[wait]".repeat(size / 34),
            ),
        ] {
            group.throughput(Throughput::Bytes(source.len() as u64));
            group.bench_with_input(BenchmarkId::new(name, size), &source, |b, source| {
                b.iter(|| {
                    black_box(keine_core::runtime::text::inline_waits(black_box(source)).count())
                });
            });
        }
    }
    group.finish();
}

fn bench_list_reads(c: &mut Criterion) {
    use keine_core::EiyashouExpr;
    use keine_core::runtime::eiyashou::evaluate;
    use std::collections::HashMap;

    let mut group = c.benchmark_group("eiyashou_list_read");
    for size in [16, 4_096, 65_536] {
        let variables = HashMap::from([(
            "items".to_owned(),
            Value::Array((0..size).map(|index| Value::Int(index as i64)).collect()),
        )]);
        let list = EiyashouExpr::Variable("items".into());
        for (name, expression) in [
            ("length", EiyashouExpr::Length(Box::new(list.clone()))),
            (
                "index",
                EiyashouExpr::Index {
                    list: Box::new(list),
                    index: Box::new(EiyashouExpr::Literal(Value::Int((size - 1) as i64))),
                },
            ),
        ] {
            group.bench_with_input(
                BenchmarkId::new(name, size),
                &expression,
                |b, expression| {
                    b.iter(|| {
                        black_box(evaluate(black_box(expression), black_box(&variables)).unwrap())
                    });
                },
            );
        }
    }
    group.finish();
}

// The hot boolean query must stay allocation-free and short-circuiting.
fn bench_presentation_blocked(c: &mut Criterion) {
    let mut group = c.benchmark_group("presentation_blocked");
    for (name, state) in [
        ("idle", State::new()),
        (
            "timed_wait",
            State {
                wait_blocking: true,
                wait_remaining: 1.0,
                ..State::new()
            },
        ),
        (
            "advance",
            State {
                waiting_for_advance: true,
                ..State::new()
            },
        ),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| black_box(black_box(&state).presentation_blocked()))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench,
    bench_markup,
    bench_list_reads,
    bench_presentation_blocked
);
criterion_main!(benches);
