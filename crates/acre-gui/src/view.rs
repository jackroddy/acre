use std::time::{Duration, Instant};

use acre::lang::Span;
use acre::render;
use acre::scene::{Animation, Point, Scene, Track};
use eframe::egui::{
    self, Color32, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2, pos2,
};

use crate::drag::{Drag, Release};
use crate::snap::{self, Bounds, Snapped};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 40.0;

/// The fraction of the pane a fitted canvas fills.
const FIT_MARGIN: f32 = 0.96;

/// How much of the pane's width and height the texture holds past each
/// edge, so a pan shows drawn content until the scene is drawn again.
const OVERDRAW: f32 = 0.25;

/// How long the view waits after the last pan or zoom before it draws
/// the scene again.
const SETTLE: Duration = Duration::from_millis(150);

/// The render pane: the scene drawn at the pane's own pixels, zoomed and
/// panned by the person.
pub struct View {
    /// Whether the canvas follows the pane's size, until the person
    /// zooms or pans.
    fit: bool,
    /// Screen points per canvas pixel.
    zoom: f32,
    /// Where the canvas's top-left corner sits, from the pane's.
    origin: Vec2,
    texture: Option<TextureHandle>,
    /// What the texture shows, to draw again only when that changes.
    drawn: Option<Drawn>,
    /// When the person last panned or zoomed.
    moved: Option<Instant>,
    /// The primary press in the pane, until it is let go.
    drag: Option<Drag>,
    /// A node let go of after a drag, and how far, drawn there until
    /// the source it was moved in comes back.
    held: Option<(Span, Point)>,
    /// Whether a dragged node snaps to other objects and the grid.
    pub snap: bool,
    /// Whether the grid is drawn and snapped to.
    pub grid: bool,
    /// The grid's spacing in canvas pixels.
    pub spacing: f64,
}

/// What the person did in the pane this frame.
pub struct Shown {
    /// Where the pane is on screen.
    pub rect: Rect,
    /// The canvas point under the pointer.
    pub hover: Option<Point>,
    /// The canvas point clicked.
    pub clicked: Option<Point>,
    /// The node that drew the object double-clicked.
    pub double_clicked: Option<Span>,
    /// The node dragged and let go of this frame, and how far it moved
    /// in canvas pixels.
    pub moved: Option<(Span, Point)>,
}

/// The view the texture was drawn for.
#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    pane: Vec2,
    pixels_per_point: f32,
    /// The longest side, in pixels, the GPU takes for a texture.
    max_side: f32,
    zoom: f32,
    origin: Vec2,
    scene: u64,
    time: f64,
    /// The node drawn moved by a drag, and how far.
    preview: Option<(Span, Point)>,
}

impl Drawn {
    /// How far past each edge of the pane the texture reaches, in points,
    /// cut back so the texture fits within `max_side`.
    fn overdraw(&self) -> Vec2 {
        let room = (Vec2::splat(self.max_side / self.pixels_per_point) - self.pane) / 2.0;
        (self.pane * OVERDRAW).min(room.max(Vec2::ZERO)).floor()
    }

    /// Texture pixels per point: the screen's, or fewer when the pane
    /// alone is wider or taller than `max_side` pixels.
    fn texture_pixels_per_point(&self) -> f32 {
        let area = self.pane + self.overdraw() * 2.0;
        self.pixels_per_point.min(self.max_side / area.max_elem())
    }
}

impl View {
    pub fn new() -> Self {
        View {
            fit: true,
            zoom: 1.0,
            origin: Vec2::ZERO,
            texture: None,
            drawn: None,
            moved: None,
            drag: None,
            held: None,
            snap: true,
            grid: false,
            spacing: 10.0,
        }
    }

    /// Stop drawing the node let go of after a drag where it was moved
    /// to.
    pub fn end_move(&mut self) {
        self.held = None;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn fit(&mut self) {
        self.fit = true;
    }

    /// Zoom by `factor` about the middle of the pane of size `pane`.
    pub fn zoom_by(&mut self, factor: f32, pane: Vec2) {
        self.zoom_about(factor, pane / 2.0);
    }

    /// Show the canvas at one screen point to a canvas pixel.
    pub fn actual_size(&mut self, pane: Vec2) {
        self.zoom_about(1.0 / self.zoom, pane / 2.0);
    }

    fn zoom_about(&mut self, factor: f32, at: Vec2) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let factor = zoom / self.zoom;
        self.origin = at - (at - self.origin) * factor;
        self.zoom = zoom;
        self.fit = false;
        self.moved = Some(Instant::now());
    }

    /// Lay the pane out in `ui`, take the person's zoom, pan and drag,
    /// and draw `scene`, whose version is `version`, at time `t`, with an
    /// outline around each box in `marks`, given in canvas pixels, and
    /// another around each object the node at `selected` draws. While a
    /// node is dragged, its objects are drawn where it is dragged to,
    /// snapped, and outlined in place of `marks`, with a guide for each
    /// snap. A double-click on no object fits the canvas to the pane.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: Option<&mut Scene>,
        version: u64,
        t: f64,
        marks: &[(Point, Point)],
        selected: Option<Span>,
    ) -> Shown {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let mut shown = Shown {
            rect,
            hover: None,
            clicked: None,
            double_clicked: None,
            moved: None,
        };
        let Some(scene) = scene else {
            self.drag = None;
            return shown;
        };
        let canvas = Vec2::new(scene.canvas.width as f32, scene.canvas.height as f32);

        let (pressed, down, pointer) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });

        // before this frame's pan and zoom, so the press lands on what
        // the last frame showed
        if pressed
            && response.contains_pointer()
            && let Some(at) = pointer
        {
            let node = scene
                .object_at(self.to_canvas(rect, at), t)
                .map(|i| scene.objects[i].source)
                .filter(|s| s.start < s.end);
            self.drag = Some(Drag::new(node, at));
        }

        if response.double_clicked() {
            let node = pointer
                .and_then(|at| scene.object_at(self.to_canvas(rect, at), t))
                .map(|i| scene.objects[i].source)
                .filter(|s| s.start < s.end);
            match node {
                Some(node) => shown.double_clicked = Some(node),
                None => self.fit = true,
            }
        }
        let pans = self.drag.as_ref().is_some_and(Drag::pans);
        if response.dragged_by(PointerButton::Middle)
            || (pans && response.dragged_by(PointerButton::Primary))
        {
            self.origin += response.drag_delta();
            self.fit = false;
            self.moved = Some(Instant::now());
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0
                && let Some(pointer) = response.hover_pos()
            {
                self.zoom_about((scroll / 200.0).exp(), pointer - rect.min);
            }
        }
        if self.fit {
            let zoom = (rect.width() / canvas.x)
                .min(rect.height() / canvas.y)
                .clamp(MIN_ZOOM, MAX_ZOOM)
                * FIT_MARGIN;
            if zoom != self.zoom {
                // the status line drew this frame's zoom before the
                // view knew it
                ui.ctx().request_repaint();
            }
            self.zoom = zoom;
            self.origin = (rect.size() - canvas * self.zoom) / 2.0;
        }

        if let Some(drag) = &mut self.drag
            && let Some(at) = pointer
        {
            drag.follow(at, self.zoom);
        }
        let mut guides = Vec::new();
        if let Some(drag) = &mut self.drag
            && let Some((node, by)) = drag.moving()
        {
            let reach = (snap::REACH / self.zoom) as f64;
            let grid = self.grid.then_some(self.spacing);
            let snapped = snap_drag(scene, node, by, t, self.snap, reach, grid);
            drag.place(snapped.by);
            guides = snapped.guides;
        }
        if !down && let Some(drag) = self.drag.take() {
            match drag.release() {
                Release::Click(at) => shown.clicked = Some(self.to_canvas(rect, at)),
                Release::Move { node, by } => {
                    self.held = Some((node, by));
                    shown.moved = Some((node, by));

                    // the caller ends the move or shows the moved
                    // source after this frame is drawn
                    ui.ctx().request_repaint();
                }
                Release::Pan => {}
            }
        }
        let preview = self.drag.as_ref().and_then(Drag::moving).or(self.held);
        let lifted: Vec<usize> = match preview {
            Some((node, by)) => {
                let lifted = drawn_by(scene, node);

                // the last group is applied last, so an extra one moves
                // the objects by `by` on the canvas whatever their own
                // groups scale them by
                let mut shift = Animation::new(Point::ZERO);
                shift.offset = Track::Constant(by);
                for &i in &lifted {
                    scene.objects[i].groups.push(shift.clone());
                }
                lifted
            }
            None => Vec::new(),
        };

        let ppp = ui.ctx().pixels_per_point();
        let wanted = Drawn {
            pane: rect.size(),
            pixels_per_point: ppp,
            max_side: ui.ctx().input(|i| i.max_texture_side) as f32,
            zoom: self.zoom,
            origin: self.origin,
            scene: version,
            time: t,
            preview,
        };
        let painter = ui.painter_at(rect);
        let mut cover = self.drawn.map(|d| self.texture_rect(rect, &d));

        // between pans and zooms the last texture is moved and scaled
        // into place, and the scene is drawn again only once the person
        // stops, or when the texture no longer covers the pane
        let settled = self.moved.is_none_or(|t| t.elapsed() >= SETTLE) && !response.dragged();
        let stale = match self.drawn {
            None => true,
            Some(d) => {
                d.scene != version
                    || d.time != t
                    || d.pane != wanted.pane
                    || d.pixels_per_point != ppp
                    || d.preview != wanted.preview
                    || (d != wanted && settled)
                    || !cover.is_some_and(|c| c.contains_rect(rect))
            }
        };
        if stale && self.draw(ui.ctx(), scene, wanted) {
            cover = Some(self.texture_rect(rect, &wanted));
        } else if self.drawn != Some(wanted) {
            ui.ctx().request_repaint_after(SETTLE);
        }
        if let (Some(texture), Some(cover)) = (&self.texture, cover) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), cover, uv, Color32::WHITE);
        }
        let to_screen = |p: Point| self.to_screen(rect, p);
        if self.grid {
            let step = snap::grid_step(self.spacing, self.zoom);
            let (lo, hi) = (
                self.to_canvas(rect, rect.min),
                self.to_canvas(rect, rect.max),
            );
            let (width, height) = (canvas.x as f64, canvas.y as f64);
            let (top, bottom) = (lo.y.max(0.0), hi.y.min(height));
            let (left, right) = (lo.x.max(0.0), hi.x.min(width));
            let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0x80, 0x80, 0x80, 0x50));
            for x in multiples(left, right, step) {
                let ends = [Point::new(x, top), Point::new(x, bottom)];
                painter.line_segment(ends.map(to_screen), stroke);
            }
            for y in multiples(top, bottom, step) {
                let ends = [Point::new(left, y), Point::new(right, y)];
                painter.line_segment(ends.map(to_screen), stroke);
            }
        }
        let lifted_marks: Vec<(Point, Point)>;
        let marks = if preview.is_some() {
            lifted_marks = lifted.iter().filter_map(|&i| scene.bounds(i, t)).collect();
            &lifted_marks
        } else {
            marks
        };
        for &(min, max) in marks {
            let mark = Rect::from_min_max(to_screen(min), to_screen(max)).expand(3.0);
            painter.rect_stroke(
                mark,
                3.0,
                Stroke::new(2.0, Color32::from_rgb(0xff, 0x9e, 0x64)),
                StrokeKind::Outside,
            );
        }

        // after the lift, so a dragged selection is outlined where it is
        // drawn, and wider than a mark, so both show on one object
        for i in selected.map_or(Vec::new(), |node| drawn_by(scene, node)) {
            let Some((min, max)) = scene.bounds(i, t) else {
                continue;
            };
            let mark = Rect::from_min_max(to_screen(min), to_screen(max)).expand(7.0);
            painter.rect_stroke(
                mark,
                4.0,
                Stroke::new(2.0, Color32::from_rgb(0x7a, 0xa2, 0xf7)),
                StrokeKind::Outside,
            );
        }

        for g in &guides {
            painter.line_segment(
                [to_screen(g.from), to_screen(g.to)],
                Stroke::new(1.0, Color32::from_rgb(0xf7, 0x76, 0x8e)),
            );
        }

        for &i in &lifted {
            scene.objects[i].groups.pop();
        }

        shown.hover = response.hover_pos().map(|p| self.to_canvas(rect, p));
        shown
    }

    /// The screen point at canvas point `p` in a pane at `rect`.
    pub fn to_screen(&self, rect: Rect, p: Point) -> Pos2 {
        rect.min + self.origin + Vec2::new(p.x as f32, p.y as f32) * self.zoom
    }

    /// The canvas point at screen point `p` in a pane at `rect`.
    fn to_canvas(&self, rect: Rect, p: Pos2) -> Point {
        let v = (p - rect.min - self.origin) / self.zoom;
        Point::new(v.x as f64, v.y as f64)
    }

    /// Draw `scene` into the texture for the view `wanted`, with the
    /// overdraw around the pane. False when the pane has no pixels.
    fn draw(&mut self, ctx: &egui::Context, scene: &Scene, wanted: Drawn) -> bool {
        let overdraw = wanted.overdraw();
        let area = wanted.pane + overdraw * 2.0;
        let ppp = wanted.texture_pixels_per_point();
        let max_side = wanted.max_side as usize;
        let pixels = [
            ((area.x * ppp).round() as usize).min(max_side),
            ((area.y * ppp).round() as usize).min(max_side),
        ];
        if pixels[0] == 0 || pixels[1] == 0 {
            return false;
        }
        let origin = (wanted.origin + overdraw) * ppp;
        let pixmap = render::draw_view(
            scene,
            wanted.time,
            pixels[0] as u32,
            pixels[1] as u32,
            (wanted.zoom * ppp) as f64,
            Point::new(origin.x as f64, origin.y as f64),
        );
        let image = egui::ColorImage::from_rgba_premultiplied(pixels, pixmap.data());
        match &mut self.texture {
            Some(texture) => texture.set(image, TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture("render", image, TextureOptions::LINEAR));
            }
        }
        self.drawn = Some(wanted);
        true
    }

    /// Where on screen the texture drawn for `drawn` goes for the current
    /// zoom and origin, in a pane at `rect`.
    fn texture_rect(&self, rect: Rect, drawn: &Drawn) -> Rect {
        // a point at x on the old screen is now at
        //   origin + (x - old origin) * zoom / old zoom
        let scale = self.zoom / drawn.zoom;
        let overdraw = drawn.overdraw();
        let min = rect.min + self.origin + (-overdraw - drawn.origin) * scale;
        Rect::from_min_size(min, (drawn.pane + overdraw * 2.0) * scale)
    }
}

/// The indices of the objects in `scene` that the node at `node` or a
/// node inside it drew.
fn drawn_by(scene: &Scene, node: Span) -> Vec<usize> {
    let inside = |s: Span| node.start <= s.start && s.end <= node.end;
    (0..scene.objects.len())
        .filter(|&i| inside(scene.objects[i].source))
        .collect()
}

/// Where the node at `node`, dragged `by` canvas pixels, goes at time
/// `t`: its objects' box snapped to the boxes of the other objects and to
/// the grid of `grid` pixels within `reach` canvas pixels when `on`, and
/// moved by whole pixels when not.
fn snap_drag(
    scene: &Scene,
    node: Span,
    by: Point,
    t: f64,
    on: bool,
    reach: f64,
    grid: Option<f64>,
) -> Snapped {
    let lifted = drawn_by(scene, node);
    let mut moving: Option<Bounds> = None;
    let mut others = Vec::new();
    for i in 0..scene.objects.len() {
        let Some((min, max)) = scene.bounds(i, t) else {
            continue;
        };
        if lifted.binary_search(&i).is_err() {
            others.push((min, max));
            continue;
        }
        moving = Some(match moving {
            None => (min, max),
            Some((a, b)) => (
                Point::new(a.x.min(min.x), a.y.min(min.y)),
                Point::new(b.x.max(max.x), b.y.max(max.y)),
            ),
        });
    }
    match moving {
        Some(moving) if on => snap::snap(moving, &others, by, reach, grid),
        _ => Snapped {
            by: Point::new(by.x.round(), by.y.round()),
            guides: Vec::new(),
        },
    }
}

/// The multiples of `step` from `lo` to `hi`.
fn multiples(lo: f64, hi: f64, step: f64) -> impl Iterator<Item = f64> {
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    (first..=last).map(move |k| k as f64 * step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(zoom: f32, origin: Vec2) -> Drawn {
        Drawn {
            pane: Vec2::new(400.0, 200.0),
            pixels_per_point: 1.0,
            max_side: 8192.0,
            zoom,
            origin,
            scene: 0,
            time: 0.0,
            preview: None,
        }
    }

    #[test]
    fn an_unmoved_texture_covers_the_pane_and_its_overdraw() {
        let mut view = View::new();
        view.zoom = 2.0;
        view.origin = Vec2::new(30.0, 40.0);
        let rect = Rect::from_min_size(pos2(10.0, 20.0), Vec2::new(400.0, 200.0));
        let cover = view.texture_rect(rect, &drawn(2.0, view.origin));
        assert_eq!(cover, rect.expand2(Vec2::new(100.0, 50.0)));
    }

    #[test]
    fn a_texture_follows_a_pan_and_scales_with_a_zoom() {
        let mut view = View::new();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        view.zoom = 1.0;
        view.origin = Vec2::new(25.0, 0.0);
        let cover = view.texture_rect(rect, &drawn(1.0, Vec2::ZERO));
        assert_eq!(cover.min, pos2(-75.0, -50.0));

        // zoomed 2x about the pane's top-left corner, the canvas origin
        // stays at the corner and everything else spreads out
        view.zoom_about(2.0, Vec2::ZERO);
        view.origin = Vec2::ZERO;
        let cover = view.texture_rect(rect, &drawn(1.0, Vec2::ZERO));
        assert_eq!(cover.min, pos2(-200.0, -100.0));
        assert_eq!(cover.size(), Vec2::new(1200.0, 600.0));
    }

    /// The left and right edges of object `i` of `src` at time 0.
    fn edges(src: &str, i: usize) -> (f64, f64) {
        let scene = acre::scene::build(&acre::lang::parse(src).unwrap(), &[], &[]).unwrap();
        let (min, max) = scene.bounds(i, 0.0).unwrap();
        (min.x, max.x)
    }

    /// `src` with object 0 dragged by `by` and snapped, written back by
    /// `move_edits`.
    fn dragged(src: &str, by: Point) -> String {
        let scene = acre::scene::build(&acre::lang::parse(src).unwrap(), &[], &[]).unwrap();
        let node = scene.objects[0].source;
        let snapped = snap_drag(&scene, node, by, 0.0, true, 6.0, None);
        let edits = acre::scene::edit::move_edits(src, node, snapped.by.x, snapped.by.y).unwrap();
        acre::scene::edit::apply(src, &edits)
    }

    #[test]
    fn a_snapped_edge_lands_on_the_edge_once_written() {
        // the other box's centre is at 42.5, and the dragged box's left
        // edge comes within 0.2 of it
        let src =
            "canvas { size 300 200; box { at 0 0; size 20 10; } box { at 34 50; size 17 20; } }";
        let out = dragged(src, Point::new(42.3, 0.4));
        assert!(out.contains("at 42.5 0;"), "{out}");
        assert_eq!(edges(&out, 0).0, 42.5);

        // a right edge at 73.333 is written to two decimals
        let src = "canvas { size 300 200; box { at 0 0; size 20 10; } box { at 40 50; size 33.333 20; } }";
        let out = dragged(src, Point::new(51.0, 0.0));
        assert!(out.contains("at 53.33 0;"), "{out}");
        assert!((edges(&out, 0).1 - edges(&out, 1).1).abs() < 0.005);
    }

    #[test]
    fn an_unsnapped_drag_moves_by_whole_pixels() {
        let src =
            "canvas { size 300 200; box { at 0 0; size 20 10; } box { at 200 150; size 20 10; } }";
        let out = dragged(src, Point::new(41.6, 20.4));
        assert!(out.contains("at 42 20;"), "{out}");
    }

    fn texture_side(d: &Drawn) -> f32 {
        ((d.pane + d.overdraw() * 2.0) * d.texture_pixels_per_point())
            .round()
            .max_elem()
    }

    #[test]
    fn a_wide_pane_gives_up_overdraw_to_fit_the_gpu() {
        // 3217 points at 2 pixels a point, 1.5x for overdraw, is 9651
        // pixels; 8192 leaves 879 points to share between the two edges
        let mut d = drawn(1.0, Vec2::ZERO);
        d.pane = Vec2::new(3217.0, 1200.0);
        d.pixels_per_point = 2.0;
        assert_eq!(d.overdraw(), Vec2::new(439.0, 300.0));
        assert_eq!(d.texture_pixels_per_point(), 2.0);
        assert!(texture_side(&d) <= 8192.0);
    }

    #[test]
    fn a_pane_wider_than_the_gpu_draws_at_fewer_pixels() {
        let mut d = drawn(1.0, Vec2::ZERO);
        d.pane = Vec2::new(5000.0, 1200.0);
        d.pixels_per_point = 2.0;
        assert_eq!(d.overdraw().x, 0.0);
        assert!(d.texture_pixels_per_point() < 2.0);
        assert_eq!(texture_side(&d), 8192.0);
    }
}
