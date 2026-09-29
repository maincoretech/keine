//! Read-only projection of the workspace's Engine process history.
use std::time::Instant;

use gpui_kit::PathBuilder;

use super::*;
use crate::preview::performance::{HISTORY_WINDOW, LifecycleSample, PerformanceSnapshot, Sample};

pub(super) fn render(controller: &PreviewController, scroll: &ScrollHandle) -> AnyElement {
    let snapshot = controller.snapshot();
    let history = controller.performance();
    let now = Instant::now();
    let current = history.samples.back().filter(|sample| {
        history.process.is_some()
            && sample.process == history.process
            && now.saturating_duration_since(sample.at) <= Duration::from_secs(2)
    });
    let unavailable = if history.pid.is_some() {
        "Unavailable"
    } else {
        "—"
    };
    let cpu = current
        .and_then(|sample| sample.cpu_percent)
        .map_or_else(|| unavailable.to_owned(), |value| format!("{value:.0}%"));
    let memory = current
        .and_then(|sample| sample.resident_bytes)
        .map_or_else(|| unavailable.to_owned(), format_memory);
    let peak = history
        .observed_peak
        .map_or_else(|| "—".to_owned(), format_memory);
    let content = div()
        .flex()
        .flex_col()
        .p_3()
        .gap_3()
        .child(view::section_label("ENGINE PREVIEW"))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(lifecycle_color(&snapshot.lifecycle)))
                        .child(lifecycle_name(&snapshot.lifecycle)),
                )
                .child(
                    div().text_xs().text_color(rgb(MUTED)).child(
                        history
                            .pid
                            .map_or_else(String::new, |pid| format!("PID {pid}")),
                    ),
                ),
        )
        .child(lifecycle_band(
            history.lifecycle.iter().cloned().collect(),
            now,
        ))
        .child(view::property_row("CPU", cpu))
        .child(sparkline(
            &history,
            now,
            |sample| sample.cpu_percent,
            PRIMARY,
        ))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("100% = 1 logical core"),
        )
        .child(view::property_row("Memory · RSS", memory))
        .child(sparkline(
            &history,
            now,
            |sample| sample.resident_bytes.map(|value| value as f64),
            0x9dc9a5,
        ))
        .child(view::property_row("Observed peak", peak))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Last 30s · sampled every 500ms"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Peak is the highest sampled RSS in this Engine process."),
        );
    view::vertical_overflow_view("performance-scroll", scroll, content)
}

fn format_memory(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
}

fn lifecycle_name(state: &PreviewLifecycle) -> &'static str {
    match state {
        PreviewLifecycle::Off => "Off",
        PreviewLifecycle::Starting => "Starting",
        PreviewLifecycle::Running => "Running",
        PreviewLifecycle::Failed(_) => "Failed",
    }
}

fn lifecycle_color(state: &PreviewLifecycle) -> u32 {
    match state {
        PreviewLifecycle::Off => MUTED,
        PreviewLifecycle::Starting => 0xe0bd78,
        PreviewLifecycle::Running => PRIMARY,
        PreviewLifecycle::Failed(_) => 0xf09090,
    }
}

fn time_x(at: Instant, now: Instant, width: f32) -> f32 {
    (1. - now.saturating_duration_since(at).as_secs_f32() / HISTORY_WINDOW.as_secs_f32())
        .clamp(0., 1.)
        * width
}

fn lifecycle_band(events: Vec<LifecycleSample>, now: Instant) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            for (index, event) in events.iter().enumerate() {
                let start = time_x(event.at, now, width);
                let end = events
                    .get(index + 1)
                    .map_or(width, |next| time_x(next.at, now, width));
                if end > start {
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + gpui_kit::point(px(start), px(0.)),
                            size(px(end - start), bounds.size.height),
                        ),
                        rgb(lifecycle_color(&event.state)).opacity(0.4),
                    ));
                }
            }
        },
    )
    .w_full()
    .h(px(5.))
    .into_any_element()
}

fn sparkline(
    history: &PerformanceSnapshot,
    now: Instant,
    value: fn(&Sample) -> Option<f64>,
    color: u32,
) -> AnyElement {
    let samples: Vec<_> = history
        .samples
        .iter()
        .filter(|sample| now.saturating_duration_since(sample.at) <= HISTORY_WINDOW)
        .map(|sample| (sample.at, sample.process, value(sample)))
        .collect();
    let max = samples
        .iter()
        .filter_map(|(_, _, value)| *value)
        .fold(1., f64::max);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            if width <= 0. || height <= 4. {
                return;
            }
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + gpui_kit::point(px(0.), px(height - 1.)),
                    size(bounds.size.width, px(1.)),
                ),
                rgb(MUTED).opacity(0.15),
            ));
            let mut previous = None;
            let mut path = PathBuilder::stroke(px(1.5));
            for &(at, process, value) in &samples {
                let Some(value) = value else {
                    previous = None;
                    continue;
                };
                let point = bounds.origin
                    + gpui_kit::point(
                        px(time_x(at, now, width)),
                        px(height - 2. - (value / max) as f32 * (height - 4.)),
                    );
                let contiguous = previous.is_some_and(|(previous_at, previous_process)| {
                    previous_process == process
                        && at.saturating_duration_since(previous_at) <= Duration::from_secs(2)
                });
                if contiguous {
                    path.line_to(point);
                } else {
                    path.move_to(point);
                }
                // A first sample is still visible before a line segment exists.
                window.paint_quad(fill(Bounds::new(point, size(px(1.5), px(1.5))), rgb(color)));
                previous = Some((at, process));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(color));
            }
        },
    )
    .w_full()
    .h(px(48.))
    .into_any_element()
}
