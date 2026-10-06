//! Benchmark-only schedule windows. These are elapsed wall times, not
//! exclusive function CPU times; parallel and unassigned work can overlap.
use super::*;

#[derive(Resource, Default)]
pub(super) struct UpdateClock(Option<Instant>);

fn begin(mut clock: ResMut<UpdateClock>) {
    clock.0 = Some(Instant::now());
}

pub(super) fn checkpoint<const N: usize>(
    mut clock: ResMut<UpdateClock>,
    config: Res<RuntimeCaptureConfig>,
    mut state: ResMut<RuntimeCaptureState>,
) {
    let now = Instant::now();
    let Some(start) = clock.0.replace(now) else {
        return;
    };
    let first = config
        .samples
        .0
        .lock()
        .expect("capture lock poisoned")
        .first_frame;
    let Some(first) = first else {
        return;
    };
    let elapsed = now.duration_since(first).as_secs_f32();
    if state.finished
        || elapsed < config.warmup_seconds
        || elapsed > config.warmup_seconds + config.sample_seconds
    {
        return;
    }
    let path = [
        "update/script/elapsed_wall",
        "update/scene/elapsed_wall",
        "update/viewport/elapsed_wall",
        "update/ui/elapsed_wall",
    ][N];
    state
        .render_timings
        .entry(path.to_owned())
        .or_insert_with(|| PassSamples {
            suffix: "ms".into(),
            ..Default::default()
        })
        .record(now, now.duration_since(start).as_secs_f64() * 1000.0);
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<UpdateClock>().add_systems(
        Update,
        (
            begin.before(GameSystemSet::Input),
            checkpoint::<0>
                .after(GameSystemSet::Input)
                .before(GameSystemSet::Sync),
            checkpoint::<1>
                .after(GameSystemSet::Sync)
                .before(GameSystemSet::Layout),
            checkpoint::<2>
                .after(GameSystemSet::Layout)
                .before(GameSystemSet::Ui),
            checkpoint::<3>.after(GameSystemSet::Ui),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_capture_excludes_warmup_and_finished_runs() {
        let mut app = App::new();
        let samples = RenderCaptureSamples::default();
        samples.0.lock().unwrap().first_frame = Some(Instant::now());
        app.insert_resource(RuntimeCaptureConfig {
            warmup_seconds: 3.0,
            sample_seconds: 5.0,
            machine_output: false,
            target: None,
            cameras: BenchmarkCameras::Runtime,
            continuous: true,
            refresh_hz: None,
            samples,
        })
        .init_resource::<RuntimeCaptureState>()
        .configure_sets(
            Update,
            (
                GameSystemSet::Input,
                GameSystemSet::Sync,
                GameSystemSet::Layout,
                GameSystemSet::Ui,
            )
                .chain(),
        );
        install(&mut app);
        app.update();
        assert!(
            app.world()
                .resource::<RuntimeCaptureState>()
                .render_timings
                .is_empty()
        );
        app.world_mut()
            .resource_mut::<RuntimeCaptureConfig>()
            .samples
            .0
            .lock()
            .unwrap()
            .first_frame = Some(Instant::now() - std::time::Duration::from_secs(4));
        app.update();
        let state = app.world().resource::<RuntimeCaptureState>();
        assert_eq!(state.render_timings.len(), 4);
        assert!(state.render_timings.values().all(|s| s.values.len() == 1));
        app.world_mut()
            .resource_mut::<RuntimeCaptureState>()
            .finished = true;
        app.update();
        assert!(
            app.world()
                .resource::<RuntimeCaptureState>()
                .render_timings
                .values()
                .all(|s| s.values.len() == 1)
        );
    }
}
