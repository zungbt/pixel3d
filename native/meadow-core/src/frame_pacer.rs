//! Frame-rate cap, ported from the web's `src/frame-limiter.js`. The web filters animation-loop
//! ticks; natively there is no tick stream, so the app also asks when the next frame is due and
//! sleeps until then (skipping a frame under Fifo would otherwise spin the loop).

/// The caps the settings offer.
pub const FPS_CAPS: [u32; 4] = [60, 30, 20, 15];

const EARLY: f64 = 1.0; // ms: wake-ups jitter, so a frame this early still counts as on time
const STALL: f64 = 250.0; // ms: a gap this long (minimised, hitch) restarts the schedule

/// The cap in a setting or argument; anything unexpected means no cap (None).
pub fn parse_fps(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|n| FPS_CAPS.contains(n))
}

#[derive(Default)]
pub struct FramePacer {
    last: Option<f64>, // when the last drawn frame was due, ms
}

impl FramePacer {
    /// Whether a wake-up at `now` (ms) draws a frame.
    pub fn should_draw(&mut self, now: f64, fps: Option<u32>) -> bool {
        let (Some(fps), Some(last)) = (fps, self.last) else {
            self.last = Some(now);
            return true;
        };
        let interval = 1000.0 / fps as f64;
        let elapsed = now - last;
        if elapsed < interval - EARLY {
            return false;
        }
        // Step along the ideal schedule rather than jumping to now, so rounding never adds up to
        // drift; missed slots are skipped, not drawn back to back.
        self.last = Some(if elapsed > STALL { now } else { last + (((elapsed + EARLY) / interval).floor()).max(1.0) * interval });
        true
    }

    /// When the next frame is due (ms), or None with no cap (vsync paces the loop).
    pub fn next_due(&self, fps: Option<u32>) -> Option<f64> {
        Some(self.last? + 1000.0 / fps? as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Mulberry32;

    // Frames drawn when the screen ticks at hz for `seconds`, with ±jitter/2 ms timestamp noise.
    fn drawn(fps: Option<u32>, hz: u32, seconds: u32, jitter: f64) -> u32 {
        let mut pacer = FramePacer::default();
        let mut rng = Mulberry32::new(5);
        (0..hz * seconds)
            .filter(|&i| pacer.should_draw(i as f64 * 1000.0 / hz as f64 + (rng.next() - 0.5) * jitter, fps))
            .count() as u32
    }

    #[test]
    fn no_cap_draws_every_frame() {
        assert_eq!(drawn(None, 144, 2, 0.0), 288);
    }

    #[test]
    fn caps_120hz_to_30() {
        let n = drawn(Some(30), 120, 10, 0.0);
        assert!(n.abs_diff(300) <= 1, "{n}");
    }

    #[test]
    fn jittery_60hz_at_60_keeps_every_frame() {
        let n = drawn(Some(60), 60, 10, 1.0);
        assert!(n >= 598, "{n}");
    }

    #[test]
    fn no_drift_144hz_capped_to_60() {
        let n = drawn(Some(60), 144, 10, 0.0);
        assert!(n.abs_diff(600) <= 2, "{n}");
    }

    #[test]
    fn long_stall_restarts_the_schedule() {
        let mut p = FramePacer::default();
        assert!(p.should_draw(0.0, Some(30)));
        assert!(p.should_draw(5000.0, Some(30)));
        assert!(!p.should_draw(5010.0, Some(30)));
        assert!(p.should_draw(5034.0, Some(30)));
    }

    // The native loop: sleep until next_due, wake a little late, draw. An 80 ms hitch must not
    // be followed by a burst of catch-up frames.
    #[test]
    fn deadline_loop_skips_missed_slots_after_a_hitch() {
        let fps = Some(60);
        let mut p = FramePacer::default();
        let mut now = 0.0;
        let mut times = vec![];
        for i in 0..40 {
            if p.should_draw(now, fps) {
                times.push(now);
            }
            let late = if i == 10 { 80.0 } else { 0.3 }; // OS wake-up latency; one hitch
            now = p.next_due(fps).unwrap() + late;
        }
        let close_pairs = times.windows(2).filter(|w| w[1] - w[0] < 10.0).count();
        assert!(close_pairs <= 1, "{close_pairs} frames drawn back to back: {times:?}");
        // Apart from the hitch and the frame after it, frames stay one interval apart.
        let steady = times.windows(2).filter(|w| (w[1] - w[0] - 1000.0 / 60.0).abs() < 1.0).count();
        assert!(steady >= times.len() - 3, "{steady} steady gaps: {times:?}");
    }

    #[test]
    fn next_due_follows_the_schedule() {
        let mut p = FramePacer::default();
        assert_eq!(p.next_due(Some(30)), None);
        p.should_draw(100.0, Some(30));
        assert_eq!(p.next_due(None), None);
        assert!((p.next_due(Some(30)).unwrap() - (100.0 + 1000.0 / 30.0)).abs() < 1e-9);
    }

    #[test]
    fn parse_fps_only_offered_caps() {
        assert_eq!(parse_fps("60"), Some(60));
        assert_eq!(parse_fps("30"), Some(30));
        assert_eq!(parse_fps("20"), Some(20));
        assert_eq!(parse_fps("15"), Some(15));
        for bad in ["", "abc", "45", "0", "-30"] {
            assert_eq!(parse_fps(bad), None, "{bad}");
        }
    }
}
