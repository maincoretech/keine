//! Shared presentation math for editor list and Block reordering.
use super::*;
use std::time::Instant;

pub(super) const REORDER_DURATION: Duration = Duration::from_millis(200);

pub(super) struct PositionMotion {
    origins: HashMap<usize, f32>,
    destinations: HashMap<usize, f32>,
    pub(super) started: Instant,
}

impl PositionMotion {
    pub(super) fn new(positions: HashMap<usize, f32>) -> Self {
        Self {
            origins: positions.clone(),
            destinations: positions,
            started: Instant::now(),
        }
    }

    pub(super) fn position(&self, id: usize, reduce_motion: bool) -> Option<f32> {
        let destination = *self.destinations.get(&id)?;
        let progress = if reduce_motion {
            1.
        } else {
            ease_out_quint()(
                (self.started.elapsed().as_secs_f32() / REORDER_DURATION.as_secs_f32()).min(1.),
            )
        };
        Some(self.origins.get(&id).map_or(destination, |origin| {
            origin + (destination - origin) * progress
        }))
    }

    pub(super) fn animating(&self) -> bool {
        self.started.elapsed() < REORDER_DURATION
    }

    pub(super) fn retarget(&mut self, destinations: HashMap<usize, f32>, reduce_motion: bool) {
        self.origins = self
            .destinations
            .keys()
            .filter_map(|id| {
                self.position(*id, reduce_motion)
                    .map(|position| (*id, position))
            })
            .collect();
        self.destinations = destinations;
        self.started = Instant::now();
    }
}

pub(super) fn edge_scroll_velocity(y: f32, height: f32) -> f32 {
    const EDGE: f32 = 32.;
    if y < EDGE {
        420. * (1. - y / EDGE).clamp(0., 1.)
    } else if y > height - EDGE {
        -420. * (1. - (height - y) / EDGE).clamp(0., 1.)
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retarget_starts_at_the_current_visual_position() {
        let mut motion = PositionMotion::new(HashMap::from([(1, 0.), (2, 38.)]));
        motion.retarget(HashMap::from([(1, 38.), (2, 0.)]), false);
        motion.started -= REORDER_DURATION / 2;
        let before = motion.position(1, false).unwrap();
        motion.retarget(HashMap::from([(1, 0.), (2, 38.)]), false);
        assert!((motion.position(1, false).unwrap() - before).abs() < 0.1);
        assert_eq!(motion.position(1, true), Some(0.));
        motion.started -= REORDER_DURATION;
        assert!(!motion.animating());
        assert_eq!(motion.position(2, false), Some(38.));
    }
}
