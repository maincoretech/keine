//! Rollback checkpoint cost under the stress scenario from the performance
//! plan: 100 sprites, 1000 local variables, 200 recorded checkpoints.

use std::sync::OnceLock;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use keine_core::model::state::{Dialogue, Sprite};
use keine_core::{
    Action, BlendMode, FilmEffects, Position, Program, SpriteLayout, SpriteTransform, State,
    Transition, Value, VisualFilter,
};

const SPRITES: usize = 100;
const VARS: usize = 1_000;
const CHECKPOINTS: usize = 200;

fn program() -> &'static Program {
    static PROGRAM: OnceLock<Program> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        Program::from_scenes([(
            "bench".to_string(),
            (0..CHECKPOINTS + 1).map(|_| Action::Comment).collect(),
        )])
    })
}

fn sprite(id: usize) -> Sprite {
    Sprite {
        image: format!("figure/s{id}.webp"),
        position: Position::left(0.0),
        layout: SpriteLayout::default(),
        transition_progress: 1.0,
        transition: Transition::Instant,
        entering: true,
        transition_offset_x: 0.0,
        transition_blocking: false,
        transform: SpriteTransform::default(),
        transform_animation: None,
        position_animation: None,
        keyframe_animation: None,
        filter: VisualFilter::default(),
        films: FilmEffects::default(),
        animation: None,
        z_index: 0,
        blend: BlendMode::Alpha,
        camera_distance: None,
    }
}

fn stressed_state() -> State {
    let mut state = State::new();
    state.current_scene = "bench".to_string();
    state.install_program(program().clone());
    for i in 0..SPRITES {
        state.sprites.insert(format!("s{i}"), sprite(i));
    }
    for i in 0..VARS {
        state.vars.insert(format!("v{i}"), Value::Int(i as i64));
    }
    state.dialogue = Some(Dialogue {
        speaker: "benchmark".to_string(),
        text: "checkpoint".to_string(),
        markup: "checkpoint".to_string(),
        visible_chars: 10,
        pauses: Vec::new(),
        vocal: None,
        volume: 1.0,
        auto_advance: false,
    });
    state
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("rollback");
    group.bench_function(format!("record_{CHECKPOINTS}_checkpoints"), |b| {
        b.iter(|| {
            let mut state = stressed_state();
            for i in 0..CHECKPOINTS {
                state.record_dialogue(i);
            }
            black_box(state.backlog.len());
        });
    });
    group.bench_function(format!("record_{CHECKPOINTS}_mutating_checkpoints"), |b| {
        b.iter(|| {
            let mut state = stressed_state();
            for i in 0..CHECKPOINTS {
                state
                    .vars
                    .insert("changing".to_string(), Value::Int(i as i64));
                state.record_dialogue(i);
            }
            black_box(state.backlog.len());
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
