use std::time::Instant;

use acre::scene::Section;

/// The time in a scene that the viewer shows, and whether it is playing.
pub struct Playback {
    /// The scene's length in seconds.
    duration: f64,
    /// The time shown while paused, and the time play started from while
    /// playing.
    base: f64,
    /// When play started from `base`, or `None` while paused.
    //
    // the time is worked out from this each frame and never stepped, so
    // a frame that arrives late still shows the right instant
    started: Option<Instant>,
    looping: bool,
}

impl Playback {
    /// Paused at 0 with looping on.
    pub fn new() -> Self {
        Playback {
            duration: 0.0,
            base: 0.0,
            started: None,
            looping: true,
        }
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn playing(&self) -> bool {
        self.started.is_some()
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// The time shown at `now`, which stops play when it reaches the end
    /// with looping off.
    pub fn time(&mut self, now: Instant) -> f64 {
        let t = self.at(now);
        if self.started.is_some() && !self.looping && t >= self.duration {
            self.base = self.duration;
            self.started = None;
        }
        t
    }

    fn at(&self, now: Instant) -> f64 {
        let Some(started) = self.started else {
            return self.base;
        };
        let t = self.base + now.saturating_duration_since(started).as_secs_f64();
        if self.looping {
            t % self.duration
        } else {
            t.min(self.duration)
        }
    }

    /// Play from the time shown, or from 0 when that is the end and
    /// looping is off.
    pub fn play(&mut self, now: Instant) {
        if self.started.is_some() || self.duration <= 0.0 {
            return;
        }
        if !self.looping && self.base >= self.duration {
            self.base = 0.0;
        }
        self.started = Some(now);
    }

    pub fn pause(&mut self, now: Instant) {
        self.base = self.at(now);
        self.started = None;
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.playing() {
            self.pause(now);
        } else {
            self.play(now);
        }
    }

    pub fn set_looping(&mut self, now: Instant, looping: bool) {
        self.rebase(now, self.at(now));
        self.looping = looping;
    }

    /// Show time `t`, clamped to the scene, and keep playing from it when
    /// playing.
    pub fn seek(&mut self, now: Instant, t: f64) {
        self.rebase(now, t.clamp(0.0, self.duration));
    }

    /// Pause, and move `frames` frames at `fps` from the frame nearest
    /// the time shown.
    pub fn step(&mut self, now: Instant, frames: i64, fps: u32) {
        self.pause(now);
        let fps = fps.max(1) as f64;
        let frame = (self.base * fps).round() as i64 + frames;
        self.base = (frame as f64 / fps).clamp(0.0, self.duration);
    }

    /// Take the length of a rebuilt scene, keeping the time shown, clamped
    /// to it, and keeping play unless the scene has no length.
    pub fn set_duration(&mut self, now: Instant, duration: f64) {
        let t = self.at(now).min(duration);
        self.duration = duration;
        self.rebase(now, t);
        if duration <= 0.0 {
            self.started = None;
        }
    }

    fn rebase(&mut self, now: Instant, t: f64) {
        self.base = t;
        if self.started.is_some() {
            self.started = Some(now);
        }
    }
}

/// The index of the section that holds time `t`, or `None` before the
/// first one starts.
pub fn current(sections: &[Section], t: f64) -> Option<usize> {
    sections.iter().rposition(|s| s.start <= t)
}

/// The index of the subsection of `section` that holds time `t`, or
/// `None` before its first one starts.
pub fn current_subsection(section: &Section, t: f64) -> Option<usize> {
    section.subsections.iter().rposition(|s| s.start <= t)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use acre::scene::Subsection;

    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    fn five_seconds(now: Instant) -> Playback {
        let mut p = Playback::new();
        p.set_duration(now, 5.0);
        p
    }

    #[test]
    fn a_scene_opens_paused_at_0_with_looping_on() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        assert!(!p.playing());
        assert!(p.looping());
        assert_eq!(p.time(now + secs(3.0)), 0.0);
    }

    #[test]
    fn play_follows_the_clock_and_pause_holds_the_time() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        assert_eq!(p.time(now + secs(1.25)), 1.25);

        // a frame that comes late shows the time it comes at
        assert_eq!(p.time(now + secs(3.5)), 3.5);
        p.pause(now + secs(4.0));
        assert_eq!(p.time(now + secs(9.0)), 4.0);
        p.play(now + secs(10.0));
        assert_eq!(p.time(now + secs(10.5)), 4.5);
    }

    #[test]
    fn looping_wraps_to_0_at_the_end() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        assert_eq!(p.time(now + secs(6.0)), 1.0);
        assert_eq!(p.time(now + secs(12.0)), 2.0);
        assert!(p.playing());
    }

    #[test]
    fn without_looping_play_stops_on_the_end_and_plays_again_from_0() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.set_looping(now, false);
        p.play(now);
        assert_eq!(p.time(now + secs(7.0)), 5.0);
        assert!(!p.playing());
        assert_eq!(p.time(now + secs(8.0)), 5.0);
        p.play(now + secs(8.0));
        assert_eq!(p.time(now + secs(9.0)), 1.0);
    }

    #[test]
    fn turning_looping_off_while_playing_keeps_the_time() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        p.set_looping(now + secs(7.0), false);
        assert_eq!(p.time(now + secs(8.0)), 3.0);
    }

    #[test]
    fn a_seek_clamps_to_the_scene_and_keeps_playing() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.seek(now, 9.0);
        assert_eq!(p.time(now), 5.0);
        p.seek(now, -1.0);
        assert_eq!(p.time(now), 0.0);
        p.play(now);
        p.seek(now + secs(1.0), 3.0);
        assert_eq!(p.time(now + secs(1.5)), 3.5);
    }

    #[test]
    fn a_step_pauses_and_moves_one_frame_within_the_scene() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        p.step(now + secs(1.02), 1, 10);
        assert!(!p.playing());
        assert_eq!(p.time(now + secs(2.0)), 1.1);
        p.step(now, -1, 10);
        assert_eq!(p.time(now), 1.0);
        p.seek(now, 0.0);
        p.step(now, -1, 10);
        assert_eq!(p.time(now), 0.0);
        p.seek(now, 5.0);
        p.step(now, 1, 10);
        assert_eq!(p.time(now), 5.0);
    }

    #[test]
    fn a_reload_keeps_the_time_clamped_and_the_play_state() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        p.set_duration(now + secs(4.0), 2.5);
        assert!(p.playing());

        // clamped to the end of a looping scene, which is its start
        assert_eq!(p.time(now + secs(4.0)), 0.0);

        let mut p = five_seconds(now);
        p.seek(now, 4.0);
        p.set_duration(now, 3.0);
        assert_eq!(p.time(now), 3.0);
        p.set_duration(now, 6.0);
        assert_eq!(p.time(now), 3.0);
        assert!(!p.playing());
    }

    #[test]
    fn a_still_does_not_play() {
        let now = Instant::now();
        let mut p = five_seconds(now);
        p.play(now);
        p.set_duration(now + secs(1.0), 0.0);
        assert!(!p.playing());
        assert_eq!(p.time(now + secs(2.0)), 0.0);
        p.play(now);
        assert!(!p.playing());
    }

    fn sections(starts: &[f64]) -> Vec<Section> {
        starts
            .iter()
            .map(|&start| Section {
                start,
                title: String::new(),
                subsections: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn the_current_section_is_the_last_to_start() {
        let s = sections(&[1.0, 3.0]);
        assert_eq!(current(&s, 0.5), None);
        assert_eq!(current(&s, 1.0), Some(0));
        assert_eq!(current(&s, 2.9), Some(0));
        assert_eq!(current(&s, 4.0), Some(1));
    }

    #[test]
    fn the_current_subsection_is_the_last_to_start() {
        let section = Section {
            start: 1.0,
            title: String::new(),
            subsections: [1.5, 2.0]
                .map(|start| Subsection {
                    start,
                    title: String::new(),
                })
                .to_vec(),
        };
        assert_eq!(current_subsection(&section, 1.2), None);
        assert_eq!(current_subsection(&section, 1.5), Some(0));
        assert_eq!(current_subsection(&section, 9.0), Some(1));
    }
}
