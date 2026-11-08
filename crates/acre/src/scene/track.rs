use super::primitive::{Color, Point};

/// A value that changes over time: a constant, or keyframes
/// interpolated with an easing.
#[derive(Debug, Clone, PartialEq)]
pub enum Track<T> {
    Constant(T),

    /// Keyframes in time order. Before the first and after the last
    /// the track holds that keyframe's value.
    Keyframes(Vec<Keyframe<T>>),
}

/// A value at an instant, and how the track eases into it from the
/// keyframe before.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe<T> {
    pub time: f64,
    pub value: T,
    pub easing: Easing,
}

/// How the interpolation parameter runs from 0 to 1 between two
/// keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,

    /// Keep the earlier value until the keyframe's time.
    Hold,
}

impl Easing {
    pub fn apply(self, u: f64) -> f64 {
        match self {
            Easing::Linear => u,
            Easing::EaseIn => u * u,
            Easing::EaseOut => 1.0 - (1.0 - u) * (1.0 - u),
            Easing::EaseInOut => {
                if u < 0.5 {
                    2.0 * u * u
                } else {
                    1.0 - (-2.0 * u + 2.0).powi(2) / 2.0
                }
            }
            Easing::Hold => {
                if u < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }
}

/// A value that can be interpolated between two keyframes.
pub trait Lerp: Clone {
    /// The value at fraction `u` of the way from `a` to `b`.
    fn lerp(a: &Self, b: &Self, u: f64) -> Self;
}

impl Lerp for f64 {
    fn lerp(a: &Self, b: &Self, u: f64) -> Self {
        a + (b - a) * u
    }
}

impl Lerp for Point {
    fn lerp(a: &Self, b: &Self, u: f64) -> Self {
        Point {
            x: f64::lerp(&a.x, &b.x, u),
            y: f64::lerp(&a.y, &b.y, u),
        }
    }
}

impl Lerp for Color {
    fn lerp(a: &Self, b: &Self, u: f64) -> Self {
        let u = u as f32;
        Color {
            r: a.r + (b.r - a.r) * u,
            g: a.g + (b.g - a.g) * u,
            b: a.b + (b.b - a.b) * u,
            a: a.a + (b.a - a.a) * u,
        }
    }
}

impl Lerp for String {
    fn lerp(a: &Self, b: &Self, u: f64) -> Self {
        // text has no in-between, so it steps at the keyframe
        if u < 1.0 { a.clone() } else { b.clone() }
    }
}

impl<T> Track<T> {
    /// A track from keyframes in any order.
    pub fn keyframes(mut keyframes: Vec<Keyframe<T>>) -> Self {
        keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
        Track::Keyframes(keyframes)
    }

    /// A track that eases linearly from one value at `t0` to another at `t1`.
    pub fn from_to(t0: f64, a: T, t1: f64, b: T) -> Self {
        Self::keyframes(vec![
            Keyframe {
                time: t0,
                value: a,
                easing: Easing::Linear,
            },
            Keyframe {
                time: t1,
                value: b,
                easing: Easing::Linear,
            },
        ])
    }
}

impl<T: Lerp> Track<T> {
    /// The value at time `t`.
    pub fn at(&self, t: f64) -> T {
        match self {
            Track::Constant(v) => v.clone(),
            Track::Keyframes(keys) => sample(keys, t),
        }
    }
}

fn sample<T: Lerp>(keys: &[Keyframe<T>], t: f64) -> T {
    let (first, last) = match (keys.first(), keys.last()) {
        (Some(first), Some(last)) => (first, last),
        _ => panic!("a keyframe track needs at least one keyframe"),
    };
    if t <= first.time {
        return first.value.clone();
    }
    if t >= last.time {
        return last.value.clone();
    }
    let next = keys.partition_point(|k| k.time <= t);
    let (from, to) = (&keys[next - 1], &keys[next]);
    let span = to.time - from.time;
    let u = if span > 0.0 {
        (t - from.time) / span
    } else {
        1.0
    };
    T::lerp(&from.value, &to.value, to.easing.apply(u))
}

impl<T> From<T> for Track<T> {
    fn from(value: T) -> Self {
        Track::Constant(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f64, value: f64, easing: Easing) -> Keyframe<f64> {
        Keyframe {
            time,
            value,
            easing,
        }
    }

    #[test]
    fn constant_is_the_same_at_any_time() {
        let track = Track::Constant(3.0);
        assert_eq!(track.at(-1.0), 3.0);
        assert_eq!(track.at(100.0), 3.0);
    }

    #[test]
    fn holds_the_end_values_outside_the_keyframes() {
        let track = Track::from_to(1.0, 10.0, 2.0, 20.0);
        assert_eq!(track.at(0.0), 10.0);
        assert_eq!(track.at(1.0), 10.0);
        assert_eq!(track.at(2.0), 20.0);
        assert_eq!(track.at(5.0), 20.0);
    }

    #[test]
    fn interpolates_linearly_between_keyframes() {
        let track = Track::from_to(1.0, 10.0, 3.0, 20.0);
        assert_eq!(track.at(2.0), 15.0);
        assert_eq!(track.at(1.5), 12.5);
    }

    #[test]
    fn picks_the_right_segment_of_many() {
        let track = Track::keyframes(vec![
            key(2.0, 20.0, Easing::Linear),
            key(0.0, 0.0, Easing::Linear),
            key(1.0, 10.0, Easing::Linear),
        ]);
        assert_eq!(track.at(0.5), 5.0);
        assert_eq!(track.at(1.5), 15.0);
        assert_eq!(track.at(1.0), 10.0);
    }

    #[test]
    fn hold_steps_at_the_keyframe() {
        let track = Track::keyframes(vec![
            key(0.0, 0.0, Easing::Linear),
            key(1.0, 1.0, Easing::Hold),
        ]);
        assert_eq!(track.at(0.99), 0.0);
        assert_eq!(track.at(1.0), 1.0);
    }

    #[test]
    fn easings_meet_at_the_ends_and_order_in_the_middle() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert_eq!(easing.apply(0.0), 0.0);
            assert_eq!(easing.apply(1.0), 1.0);
        }
        assert!(Easing::EaseIn.apply(0.5) < 0.5);
        assert!(Easing::EaseOut.apply(0.5) > 0.5);
        assert_eq!(Easing::EaseInOut.apply(0.5), 0.5);
    }

    #[test]
    fn text_steps_at_the_keyframe() {
        let track = Track::from_to(0.0, "a".to_string(), 1.0, "b".to_string());
        assert_eq!(track.at(0.5), "a");
        assert_eq!(track.at(1.0), "b");
    }

    #[test]
    fn coincident_keyframes_take_the_later_value() {
        let track = Track::keyframes(vec![
            key(0.0, 0.0, Easing::Linear),
            key(1.0, 1.0, Easing::Linear),
            key(1.0, 5.0, Easing::Linear),
            key(2.0, 6.0, Easing::Linear),
        ]);
        assert_eq!(track.at(1.0), 5.0);
        assert_eq!(track.at(1.5), 5.5);
    }
}
