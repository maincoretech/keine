use std::sync::mpsc::{TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use crossbeam_channel::{bounded, select_biased};

fn legacy_polling_handoff() {
    let (sender, receiver) = sync_channel(1);
    sender.send(0_u8).unwrap();
    let (start_sender, start_receiver) = sync_channel(0);
    let consumer = thread::spawn(move || {
        start_receiver.recv().unwrap();
        receiver.recv().unwrap();
        receiver.recv().unwrap();
    });

    let event = match sender.try_send(1) {
        Err(TrySendError::Full(event)) => event,
        result => panic!("expected a full queue, got {result:?}"),
    };
    start_sender.send(()).unwrap();
    thread::sleep(Duration::from_millis(2));
    sender.send(event).unwrap();
    consumer.join().unwrap();
}

fn selectable_handoff() {
    let (sender, receiver) = bounded(1);
    sender.send(0_u8).unwrap();
    let (start_sender, start_receiver) = bounded(0);
    let consumer = thread::spawn(move || {
        start_receiver.recv().unwrap();
        receiver.recv().unwrap();
        receiver.recv().unwrap();
    });

    start_sender.send(()).unwrap();
    select_biased! {
        send(sender, 1) -> result => result.unwrap(),
    }
    consumer.join().unwrap();
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("video_queue/full_queue_handoff");
    group.sample_size(20);
    group.bench_function("legacy_2ms_poll", |b| b.iter(legacy_polling_handoff));
    group.bench_function("selectable", |b| b.iter(selectable_handoff));
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
