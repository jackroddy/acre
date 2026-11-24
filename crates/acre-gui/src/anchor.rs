use std::f64::consts::FRAC_1_SQRT_2;

use acre::lang::{Program, Span};
use acre::scene::edit::Spot;
use acre::scene::{Kind, Point, Scene, Side};

use crate::snap::Bounds;
use crate::tool;

/// How far, in screen points, the pointer reaches to land on an anchor
/// dot or on the outline of a thing.
pub const REACH: f32 = 8.0;

/// The box a line or an arrow ending on a thing by name reads, in canvas
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub node: Span,
    pub bounds: Bounds,
    /// Whether the thing is an ellipse, whose corners and points along a
    /// side lie on its curve.
    pub oval: bool,
}

/// The place on a thing an end dropped there takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub outline: Outline,
    /// The spot the end names, or `None` for the bare name, which ends
    /// on the edge facing the other end.
    pub spot: Option<Spot>,
    /// Where the spot is, or the centre for the bare name.
    pub at: Point,
}

/// The outlines of the things in `scene` at time `t` that a line can
/// end on by name, as `program` writes them.
pub fn outlines(program: &Program, scene: &Scene, t: f64) -> Vec<Outline> {
    scene
        .objects
        .iter()
        .filter(|o| !o.tag && o.lifetime.is_none_or(|l| l.contains(t)))
        .filter_map(|o| {
            let ((min, max), oval) = match &o.kind {
                Kind::Shape(s) => ((s.origin, s.origin + s.size), s.oval),
                Kind::Polygon(p) => (extent(&p.points)?, false),
                Kind::Polyline(p) => (extent(&p.points)?, false),
                Kind::Row(r) => {
                    let size = Point::new(r.width(), r.height);
                    ((r.origin, r.origin + size), false)
                }
                Kind::Matrix(m) => ((m.origin, m.origin + m.size()), false),
                Kind::Code(c) => ((c.origin, c.origin + c.extent()), false),
                _ => return None,
            };
            tool::attachable(program, o.source)?;
            let m = o.motion(t);
            Some(Outline {
                node: o.source,
                bounds: (m.apply(min), m.apply(max)),
                oval,
            })
        })
        .collect()
}

/// The box around `points`.
fn extent(points: &[Point]) -> Option<Bounds> {
    let first = *points.first()?;
    let (min, max) = points.iter().fold((first, first), |(a, b), p| {
        (
            Point::new(a.x.min(p.x), a.y.min(p.y)),
            Point::new(b.x.max(p.x), b.y.max(p.y)),
        )
    });
    Some((min, max))
}

const SIDES: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

impl Outline {
    fn centre(&self) -> Point {
        (self.bounds.0 + self.bounds.1) * 0.5
    }

    fn half(&self) -> Point {
        (self.bounds.1 - self.bounds.0) * 0.5
    }

    /// The nine dots an end snaps to: the centre, the middles of the four
    /// sides and the four corners, each with the spot it names.
    pub fn dots(&self) -> [(Option<Spot>, Point); 9] {
        let corner = |top, left| Some(Spot::Corner { top, left });
        [
            None,
            Some(Spot::Side(Side::Top)),
            Some(Spot::Side(Side::Bottom)),
            Some(Spot::Side(Side::Left)),
            Some(Spot::Side(Side::Right)),
            corner(true, true),
            corner(true, false),
            corner(false, true),
            corner(false, false),
        ]
        .map(|spot| (spot, self.at(spot)))
    }

    /// Where an end that names `spot` on the thing sits, or the centre
    /// for the bare name.
    pub fn at(&self, spot: Option<Spot>) -> Point {
        let (c, half) = (self.centre(), self.half());
        match spot {
            None => c,
            Some(Spot::Side(side)) => self.at(Some(Spot::Along(side, 0.5))),
            Some(Spot::Corner { top, left }) => {
                // an ellipse's corner is where the diagonal of its box
                // crosses it, as acre places it
                let reach = if self.oval { FRAC_1_SQRT_2 } else { 1.0 };
                let sx = if left { -1.0 } else { 1.0 };
                let sy = if top { -1.0 } else { 1.0 };
                c + Point::new(sx * half.x, sy * half.y) * reach
            }
            Some(Spot::Along(side, f)) => {
                let t = 2.0 * f - 1.0;
                let out = outward(side);
                let across = match side {
                    Side::Top | Side::Bottom => Point::new(t * half.x, 0.0),
                    Side::Left | Side::Right => Point::new(0.0, t * half.y),
                };
                let depth = if self.oval {
                    (1.0 - t * t).max(0.0).sqrt()
                } else {
                    1.0
                };
                c + across + Point::new(out.x * half.x, out.y * half.y) * depth
            }
        }
    }

    /// The side whose outline passes nearest `p`, and how far along it
    /// that point is, rounded to two decimals as an end writes it.
    fn along(&self, p: Point) -> Option<(Side, f64)> {
        let (c, half) = (self.centre(), self.half());
        let (side, f) = if self.oval && half.x > 0.0 && half.y > 0.0 {
            // the point of the ellipse on the way from its centre to p,
            // read off the side its box faces most
            let (u, v) = ((p.x - c.x) / half.x, (p.y - c.y) / half.y);
            let r = u.hypot(v);
            if r == 0.0 {
                return None;
            }
            if u.abs() >= v.abs() {
                let side = if u < 0.0 { Side::Left } else { Side::Right };
                (side, (v / r + 1.0) / 2.0)
            } else {
                let side = if v < 0.0 { Side::Top } else { Side::Bottom };
                (side, (u / r + 1.0) / 2.0)
            }
        } else {
            let (min, max) = self.bounds;
            let fraction = |a: f64, lo: f64, hi: f64| {
                if hi > lo {
                    ((a - lo) / (hi - lo)).clamp(0.0, 1.0)
                } else {
                    0.5
                }
            };
            SIDES
                .into_iter()
                .map(|side| {
                    let f = match side {
                        Side::Top | Side::Bottom => fraction(p.x, min.x, max.x),
                        Side::Left | Side::Right => fraction(p.y, min.y, max.y),
                    };
                    (side, f)
                })
                .min_by(|a, b| {
                    let d = |(side, f)| distance(p, self.at(Some(Spot::Along(side, f))));
                    d(*a).total_cmp(&d(*b))
                })?
        };
        Some((side, (f * 100.0).round() / 100.0))
    }
}

fn outward(side: Side) -> Point {
    match side {
        Side::Top => Point::new(0.0, -1.0),
        Side::Bottom => Point::new(0.0, 1.0),
        Side::Left => Point::new(-1.0, 0.0),
        Side::Right => Point::new(1.0, 0.0),
    }
}

fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Where an end at canvas point `p` lands on the things of `outlines`,
/// reaching `reach` canvas pixels: on the nearest of the smallest
/// thing's dots, or else at the nearest point of its outline, or else,
/// inside it, on the bare name. `None` when `p` is on no thing.
pub fn anchor_at(outlines: &[Outline], p: Point, reach: f64) -> Option<Anchor> {
    let near = |(min, max): Bounds| {
        min.x - reach <= p.x && p.x <= max.x + reach && min.y - reach <= p.y && p.y <= max.y + reach
    };
    let area = |o: &Outline| (o.bounds.1.x - o.bounds.0.x) * (o.bounds.1.y - o.bounds.0.y);

    // reversed, so the last drawn of two the same size comes first
    let outline = *outlines
        .iter()
        .rev()
        .filter(|o| near(o.bounds))
        .min_by(|a, b| area(a).total_cmp(&area(b)))?;
    let anchor = |spot, at| Anchor { outline, spot, at };
    let dot = outline
        .dots()
        .into_iter()
        .map(|(spot, at)| (spot, at, distance(p, at)))
        .filter(|&(_, _, d)| d <= reach)
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((spot, at, _)) = dot {
        return Some(anchor(spot, at));
    }
    if let Some((side, f)) = outline.along(p) {
        let spot = Some(Spot::Along(side, f));
        let at = outline.at(spot);
        if distance(p, at) <= reach {
            return Some(anchor(spot, at));
        }
    }
    let (min, max) = outline.bounds;
    let inside = min.x <= p.x && p.x <= max.x && min.y <= p.y && p.y <= max.y;
    inside.then(|| anchor(None, outline.centre()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::lang::parse;
    use acre::scene::{self, Connector, edit};

    fn outline(x0: f64, y0: f64, x1: f64, y1: f64, oval: bool) -> Outline {
        Outline {
            node: Span { start: 0, end: 1 },
            bounds: (Point::new(x0, y0), Point::new(x1, y1)),
            oval,
        }
    }

    fn spot_at(outlines: &[Outline], x: f64, y: f64) -> Option<Option<Spot>> {
        anchor_at(outlines, Point::new(x, y), 8.0).map(|a| a.spot)
    }

    #[test]
    fn a_box_has_nine_dots_at_its_corners_sides_and_centre() {
        let o = outline(0.0, 0.0, 40.0, 20.0, false);
        let dots = o.dots();
        assert_eq!(dots[0], (None, Point::new(20.0, 10.0)));
        assert!(dots.contains(&(Some(Spot::Side(Side::Right)), Point::new(40.0, 10.0))));
        let top_left = Some(Spot::Corner {
            top: true,
            left: true,
        });
        assert!(dots.contains(&(top_left, Point::ZERO)));
    }

    #[test]
    fn an_end_snaps_to_the_nearest_dot_within_reach() {
        let o = [outline(100.0, 100.0, 200.0, 160.0, false)];
        let corner = Some(Spot::Corner {
            top: false,
            left: false,
        });
        assert_eq!(spot_at(&o, 205.0, 163.0), Some(corner));
        assert_eq!(spot_at(&o, 153.0, 96.0), Some(Some(Spot::Side(Side::Top))));
        assert_eq!(spot_at(&o, 152.0, 128.0), Some(None));
        assert_eq!(spot_at(&o, 300.0, 300.0), None);
    }

    #[test]
    fn near_the_outline_an_end_takes_a_point_along_the_nearest_side() {
        let o = [outline(100.0, 100.0, 200.0, 160.0, false)];
        let a = anchor_at(&o, Point::new(125.4, 104.0), 8.0).unwrap();
        assert_eq!(a.spot, Some(Spot::Along(Side::Top, 0.25)));
        assert_eq!(a.at, Point::new(125.0, 100.0));
        assert_eq!(
            spot_at(&o, 197.0, 115.0),
            Some(Some(Spot::Along(Side::Right, 0.25)))
        );
    }

    #[test]
    fn inside_away_from_the_outline_an_end_takes_the_bare_name() {
        let o = [outline(100.0, 100.0, 200.0, 160.0, false)];
        assert_eq!(spot_at(&o, 130.0, 140.0), Some(None));
    }

    #[test]
    fn the_smallest_thing_under_the_pointer_takes_the_end() {
        let big = outline(0.0, 0.0, 300.0, 300.0, false);
        let small = Outline {
            node: Span { start: 5, end: 6 },
            ..outline(100.0, 100.0, 140.0, 140.0, false)
        };
        let a = anchor_at(&[small, big], Point::new(120.0, 120.0), 8.0).unwrap();
        assert_eq!(a.outline.node, small.node);
    }

    #[test]
    fn an_ellipses_dots_and_points_along_lie_on_its_curve() {
        let o = outline(0.0, 0.0, 100.0, 60.0, true);
        let on_curve = |p: Point| {
            let (u, v) = ((p.x - 50.0) / 50.0, (p.y - 30.0) / 30.0);
            (u * u + v * v - 1.0).abs() < 1e-9
        };
        for (spot, at) in o.dots().into_iter().skip(1) {
            assert!(on_curve(at), "{spot:?} at {at:?}");
        }
        let a = anchor_at(&[o], Point::new(70.0, 2.0), 8.0).unwrap();
        assert!(matches!(a.spot, Some(Spot::Along(Side::Top, _))));
        assert!(on_curve(a.at), "{:?}", a.at);
    }

    /// Where acre draws end `end` of the arrow `src` ends with, after
    /// that end is written as `anchor` names it on thing `a`.
    fn drawn_end(src: &str, anchor: &Anchor) -> Point {
        let program = parse(src).unwrap();
        let arrow = program.nodes[0].children.last().unwrap().span;
        let end = match anchor.spot {
            None => edit::EndAt::Name("a".into()),
            Some(spot) => edit::EndAt::On("a".into(), spot),
        };
        let out = edit::apply(src, &edit::end_edits(src, arrow, "to", &end).unwrap());
        let scene = scene::build(&parse(&out).unwrap(), &[], &[]).unwrap();
        let Some(Kind::Connector(Connector { to, .. })) = scene.objects.last().map(|o| &o.kind)
        else {
            panic!("the arrow is the last object");
        };
        *to
    }

    #[test]
    fn an_end_written_from_an_anchor_lands_on_its_dot() {
        for kind in ["box", "ellipse"] {
            let src = format!(
                "canvas {{ size 300 200; {kind} a {{ at 40 50; size 100 60; }} \
                 arrow {{ from 250 190; to 0 0; }} }}"
            );
            let program = parse(&src).unwrap();
            let scene = scene::build(&program, &[], &[]).unwrap();
            let outlines = outlines(&program, &scene, 0.0);
            assert_eq!(outlines.len(), 1);
            let o = outlines[0];
            for (spot, at) in o.dots().into_iter().skip(1) {
                let anchor = Anchor {
                    outline: o,
                    spot,
                    at,
                };
                let to = drawn_end(&src, &anchor);
                assert!(distance(to, at) < 1e-6, "{kind} {spot:?}: {to:?} {at:?}");
            }
            let along = anchor_at(&outlines, Point::new(70.0, 52.0), 8.0).unwrap();
            assert!(matches!(along.spot, Some(Spot::Along(..))), "{along:?}");
            let to = drawn_end(&src, &along);
            assert!(
                distance(to, along.at) < 1e-6,
                "{kind}: {to:?} {:?}",
                along.at
            );
        }
    }

    #[test]
    fn a_repeat_copy_text_or_line_has_no_outline() {
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   repeat i 0 1 { box c { at 100 10; size 20 20; } } \
                   text { at 50 150; text \"t\"; } line { from 0 0; to 9 9; } \
                   polygon p { points 200 100 260 100 230 160; } }";
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let outlines = outlines(&program, &scene, 0.0);
        let bounds: Vec<Bounds> = outlines.iter().map(|o| o.bounds).collect();
        assert_eq!(
            bounds,
            [
                (Point::new(10.0, 10.0), Point::new(30.0, 30.0)),
                (Point::new(200.0, 100.0), Point::new(260.0, 160.0)),
            ]
        );
    }
}
