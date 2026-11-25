use std::time::{Duration, Instant};

use acre::lang::Span;
use acre::render;
use acre::scene::edit::Siblings;
use acre::scene::{Animation, Point, Scene, Track};
use eframe::egui::{
    self, Color32, CursorIcon, PointerButton, Popup, Pos2, Rect, Sense, SetOpenCommand, Shape,
    Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, pos2,
};

use crate::anchor::{self, Anchor, Outline};
use crate::arrange;
use crate::drag::{Drag, Release};
use crate::handle::{self, Edge, Grip, Reshape, Target};
use crate::reorder;
use crate::selection;
use crate::snap::{self, Bounds, Guide, Snapped};
use crate::tool::{self, Tool};

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
    /// The nodes let go of after a drag, and how far, drawn there until
    /// the source they were moved in comes back.
    held: Option<(Vec<Span>, Point)>,
    /// Whether dragged nodes snap to other objects and the grid.
    pub snap: bool,
    /// Whether the grid is drawn and snapped to.
    pub grid: bool,
    /// The grid's spacing in canvas pixels.
    pub spacing: f64,
    /// Whether the view refuses every edit to the source.
    pub locked: bool,
    pub tool: Tool,
    /// The points placed so far of the polygon or polyline being drawn,
    /// in canvas pixels.
    pub path: Vec<Point>,
    /// The one selected node that shows handles.
    pub target: Option<Target>,
    /// The things an end of a line can go on, while one is drawn or
    /// moved.
    pub outlines: Vec<Outline>,
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
    /// The nodes dragged and let go of this frame, and how far they
    /// moved in canvas pixels.
    pub moved: Option<(Vec<Span>, Point)>,
    /// The selection box let go of this frame, in canvas pixels.
    pub boxed: Option<Bounds>,
    /// Where a drag that draws a new node ran from and to, in canvas
    /// pixels, unsnapped.
    pub created: Option<(Point, Point)>,
    /// The node that drew the object right-clicked.
    pub right_clicked: Option<Span>,
    /// What was chosen from the menu of a right-clicked object.
    pub chosen: Option<MenuItem>,
    /// The change a handle dragged and let go of this frame makes.
    pub reshaped: Option<Reshape>,
    /// The one node a press this frame drags.
    pub pressed: Option<Span>,
    /// The child of a stack or grid dragged and let go of this frame,
    /// the sibling it goes before, or `None` for after the last, and its
    /// index among them after the move.
    pub reordered: Option<(Span, Option<Span>, usize)>,
}

/// An entry in the menu of a right-clicked object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuItem {
    Delete,
    Duplicate,
}

/// The view the texture was drawn for.
#[derive(Clone, PartialEq)]
struct Drawn {
    pane: Vec2,
    pixels_per_point: f32,
    /// The longest side, in pixels, the GPU takes for a texture.
    max_side: f32,
    zoom: f32,
    origin: Vec2,
    scene: u64,
    time: f64,
    /// The nodes drawn moved by a drag, and how far.
    preview: Option<(Vec<Span>, Point)>,
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
            locked: false,
            tool: Tool::Select,
            path: Vec::new(),
            target: None,
            outlines: Vec::new(),
        }
    }

    /// Make the drag just pressed on one child of a stack or grid, whose
    /// siblings are `siblings`, reorder it among them.
    pub fn reorder(&mut self, siblings: Siblings) {
        if let Some(drag) = &mut self.drag {
            drag.reorder(siblings);
        }
    }

    /// Stop drawing the nodes let go of after a drag where they were
    /// moved to.
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
    /// outline around each object the nodes at `selected` draw. While
    /// nodes are dragged, their objects are drawn where they are dragged
    /// to, snapped, and outlined, with a guide for each snap. A drag that reorders a child of a stack or grid outlines
    /// it at the pointer and marks the place it would land. A drag on
    /// empty canvas draws a selection box. A
    /// double-click on no object fits the canvas to the pane. With a tool
    /// other than Select, a press draws a new node, outlined as it goes.
    /// A right-click on an object opens its menu. With the Select tool,
    /// the node in `target` shows handles, and a drag on one draws the outline it
    /// makes, snapped. With the Line and Arrow tools, and while the end of
    /// a line is dragged, the thing under the pointer shows the dots an
    /// end snaps to.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: Option<&mut Scene>,
        version: u64,
        t: f64,
        selected: &[Span],
    ) -> Shown {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let mut shown = Shown {
            rect,
            hover: None,
            clicked: None,
            double_clicked: None,
            moved: None,
            boxed: None,
            created: None,
            right_clicked: None,
            chosen: None,
            reshaped: None,
            pressed: None,
            reordered: None,
        };
        let Some(scene) = scene else {
            self.drag = None;
            return shown;
        };
        let canvas = Vec2::new(scene.canvas.width as f32, scene.canvas.height as f32);
        let target = self.target.clone().filter(|_| self.tool == Tool::Select);
        let target = target.as_ref();
        let handle_reach = ((handle::HANDLE + 2.0) / self.zoom) as f64;

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
            let p = self.to_canvas(rect, at);
            let grip = target.and_then(|target| Some((target, target.grip_at(p, handle_reach)?)));
            let nodes =
                node_at(scene, p, t).map_or(Vec::new(), |node| selection::dragged(selected, node));
            if let [node] = nodes[..]
                && self.tool == Tool::Select
                && grip.is_none()
                && !self.locked
            {
                shown.pressed = Some(node);
            }
            self.drag = Some(press(self.tool, self.locked, grip, nodes, at));
        }
        self.context_menu(&response, scene, rect, t, &mut shown);

        if response.double_clicked() && self.tool == Tool::Select {
            let node = pointer.and_then(|at| node_at(scene, self.to_canvas(rect, at), t));
            match node {
                Some(node) => shown.double_clicked = Some(node),
                None => self.fit = true,
            }
        }
        if response.dragged_by(PointerButton::Middle) {
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
            && let Some((nodes, by)) = drag.moving()
        {
            let reach = (snap::REACH / self.zoom) as f64;
            let grid = self.grid.then_some(self.spacing);
            let snapped = snap_drag(scene, nodes, by, t, self.snap, reach, grid);
            drag.place(snapped.by);
            guides = snapped.guides;
        }
        let place = pointer.and_then(|at| self.aim_reorder(scene, t, self.to_canvas(rect, at)));
        if let Some(drag) = &mut self.drag
            && let Some((target, grip, by)) = drag.gripping()
            && !matches!(grip, Grip::End(_))
            && let Some(at) = target.grip_point(grip)
        {
            let others: Vec<Bounds> = others(scene, target.node(), t);
            let reach = (snap::REACH / self.zoom) as f64;
            let grid = self.grid.then_some(self.spacing);
            let axes = handle::axes(grip);
            let (to, snaps) = handle::snap_handle(at + by, axes, &others, reach, grid, self.snap);
            drag.place(to - at);
            guides = snaps;
        }
        if !down && let Some(drag) = self.drag.take() {
            match drag.release() {
                Release::Click(at) => shown.clicked = Some(self.to_canvas(rect, at)),
                Release::Move { nodes, by } => {
                    self.held = Some((nodes.clone(), by));
                    shown.moved = Some((nodes, by));

                    // the caller ends the move or shows the moved
                    // source after this frame is drawn
                    ui.ctx().request_repaint();
                }
                Release::Select(from, to) => {
                    let (a, b) = (self.to_canvas(rect, from), self.to_canvas(rect, to));
                    shown.boxed = Some((
                        Point::new(a.x.min(b.x), a.y.min(b.y)),
                        Point::new(a.x.max(b.x), a.y.max(b.y)),
                    ));
                }
                Release::Create(from, to) => {
                    shown.created = Some((self.to_canvas(rect, from), self.to_canvas(rect, to)));
                }
                Release::Grip { target, grip, by } => shown.reshaped = target.reshape(grip, by),
                Release::Reorder {
                    node,
                    before,
                    index,
                } => shown.reordered = Some((node, before, index)),
            }
        }
        let preview = match self.drag.as_ref().and_then(Drag::moving) {
            Some((nodes, by)) => Some((nodes.to_vec(), by)),
            None => self.held.clone(),
        };
        let lifted: Vec<usize> = match &preview {
            Some((nodes, by)) => {
                let lifted = drawn_by(scene, nodes);

                // the last group is applied last, so an extra one moves
                // the objects by `by` on the canvas whatever their own
                // groups scale them by
                let mut shift = Animation::new(Point::ZERO);
                shift.offset = Track::Constant(*by);
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
        let mut cover = self.drawn.as_ref().map(|d| self.texture_rect(rect, d));

        // between pans and zooms the last texture is moved and scaled
        // into place, and the scene is drawn again only once the person
        // stops, or when the texture no longer covers the pane
        let settled = self.moved.is_none_or(|t| t.elapsed() >= SETTLE) && !response.dragged();
        let stale = match &self.drawn {
            None => true,
            Some(d) => {
                d.scene != version
                    || d.time != t
                    || d.pane != wanted.pane
                    || d.pixels_per_point != ppp
                    || d.preview != wanted.preview
                    || (*d != wanted && settled)
                    || !cover.is_some_and(|c| c.contains_rect(rect))
            }
        };
        if stale && self.draw(ui.ctx(), scene, &wanted) {
            cover = Some(self.texture_rect(rect, &wanted));
        } else if self.drawn.as_ref() != Some(&wanted) {
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
        for (min, max) in lifted.iter().filter_map(|&i| scene.bounds(i, t)) {
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
        for i in drawn_by(scene, selected) {
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

        if let Some(line) = place {
            let orange = Color32::from_rgb(0xff, 0x9e, 0x64);
            painter.line_segment(line.map(to_screen), Stroke::new(3.0, orange));
        }
        if let Some((node, _, by)) = self.drag.as_ref().and_then(Drag::reordering)
            && let Some((min, max)) = arrange::bounds(scene, node, t)
        {
            let outline = Rect::from_min_max(to_screen(min + by), to_screen(max + by));
            painter.rect_stroke(
                outline,
                3.0,
                Stroke::new(2.0, Color32::from_rgb(0x7a, 0xa2, 0xf7)),
                StrokeKind::Outside,
            );
        }

        if let Some((from, to)) = self.drag.as_ref().and_then(Drag::boxed) {
            let blue = Color32::from_rgb(0x7a, 0xa2, 0xf7);
            let area = Rect::from_two_pos(from, to);
            painter.rect_filled(area, 0.0, blue.gamma_multiply(0.15));
            painter.rect_stroke(area, 0.0, Stroke::new(1.0, blue), StrokeKind::Inside);
        }

        let hover = response.hover_pos().map(|p| self.to_canvas(rect, p));
        guides.extend(self.draw_preview(&painter, rect, scene, t, hover));
        if let Some(target) = target {
            let gripped = self.drag.as_ref().and_then(Drag::gripping);
            let dragged = gripped.and_then(|(gripped, grip, by)| {
                let Grip::End(_) = grip else {
                    return reshaped(gripped, grip, by);
                };
                let from = gripped.grip_point(grip)?;
                let (to, anchor, snaps) = self.end_point(scene, from + by, t);
                if let Some(anchor) = anchor {
                    self.draw_anchor(&painter, rect, &anchor);
                }
                guides.extend(snaps);
                reshaped(gripped, grip, to - from)
            });
            self.draw_handles(&painter, rect, dragged.as_ref().unwrap_or(target));
            let grip = match (gripped, hover) {
                (Some((_, grip, _)), _) => Some(grip),
                (None, Some(p)) if self.drag.is_none() => target.grip_at(p, handle_reach),
                _ => None,
            };
            if let Some(grip) = grip {
                ui.ctx().set_cursor_icon(cursor(grip));
            }
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

        shown.hover = hover;
        shown
    }

    /// Open a menu with Delete and Duplicate on a right-click on an
    /// object of `scene` at time `t` in the pane at `rect`, whose
    /// response is `response`, and note on `shown` the node and the
    /// entry chosen.
    fn context_menu(
        &self,
        response: &egui::Response,
        scene: &Scene,
        rect: Rect,
        t: f64,
        shown: &mut Shown,
    ) {
        let open = if response.secondary_clicked() {
            shown.right_clicked = response
                .interact_pointer_pos()
                .and_then(|at| node_at(scene, self.to_canvas(rect, at), t));
            Some(SetOpenCommand::Bool(shown.right_clicked.is_some()))
        } else if response.clicked() {
            Some(SetOpenCommand::Bool(false))
        } else {
            None
        };
        Popup::context_menu(response).open_memory(open).show(|ui| {
            for (item, label) in [
                (MenuItem::Delete, "Delete"),
                (MenuItem::Duplicate, "Duplicate"),
            ] {
                if ui
                    .add_enabled(!self.locked, egui::Button::new(label))
                    .clicked()
                {
                    shown.chosen = Some(item);
                    ui.close();
                }
            }
        });
    }

    /// Canvas point `p` snapped to the edges and centres of the objects
    /// of `scene` at time `t` and to the grid, as a dragged object snaps,
    /// or rounded to whole pixels when snapping is off, with a guide for
    /// each snap.
    pub fn snap_point(&self, scene: &Scene, p: Point, t: f64) -> (Point, Vec<Guide>) {
        let p = Point::new(p.x.round(), p.y.round());
        if !self.snap {
            return (p, Vec::new());
        }
        let others: Vec<Bounds> = (0..scene.objects.len())
            .filter_map(|i| scene.bounds(i, t))
            .collect();
        let reach = (snap::REACH / self.zoom) as f64;
        let grid = self.grid.then_some(self.spacing);
        let snapped = snap::snap((p, p), &others, Point::ZERO, reach, grid);
        (p + snapped.by, snapped.guides)
    }

    /// While the drag reorders a child of a stack or grid, with the
    /// pointer at canvas point `p`, aim it at the place among its
    /// siblings nearest `p`, and give the line that marks the place when
    /// the child would move.
    fn aim_reorder(&mut self, scene: &Scene, t: f64, p: Point) -> Option<[Point; 2]> {
        let drag = self.drag.as_mut()?;
        let (node, siblings, _) = drag.reordering()?;
        let boxes: Vec<Option<Bounds>> = siblings
            .children
            .iter()
            .map(|&child| arrange::bounds(scene, child, t))
            .collect();
        let from = siblings.children.iter().position(|&c| c == node)?;
        let (slot, line) = reorder::slot(siblings.layout, &boxes, p)?;
        drag.aim(slot);
        (slot != from && slot != from + 1).then_some(line)
    }

    /// Draw the outline of the node the tool is drawing, with the
    /// pointer at canvas point `hover`, and give the guides of its snaps.
    fn draw_preview(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        scene: &Scene,
        t: f64,
        hover: Option<Point>,
    ) -> Vec<Guide> {
        let stroke = Stroke::new(1.5, Color32::from_rgb(0x7a, 0xa2, 0xf7));
        let to_screen = |p: Point| self.to_screen(rect, p);
        let mut guides = Vec::new();
        let line = matches!(self.tool, Tool::Line | Tool::Arrow);
        if line
            && self.drag.is_none()
            && let Some(hover) = hover
            && let (_, Some(anchor), _) = self.end_point(scene, hover, t)
        {
            self.draw_anchor(painter, rect, &anchor);
        }
        if let Some((from, to)) = self.drag.as_ref().and_then(Drag::creating) {
            let (from, to) = (self.to_canvas(rect, from), self.to_canvas(rect, to));
            let (from, to) = if line {
                let (from, from_anchor, _) = self.end_point(scene, from, t);
                let (to, to_anchor, snaps) = self.end_point(scene, to, t);
                for anchor in [from_anchor, to_anchor].iter().flatten() {
                    self.draw_anchor(painter, rect, anchor);
                }
                guides = snaps;
                (from, to)
            } else {
                let (from, _) = self.snap_point(scene, from, t);
                let (to, snaps) = self.snap_point(scene, to, t);
                guides = snaps;
                (from, to)
            };
            let (a, b) = (to_screen(from), to_screen(to));
            match self.tool {
                Tool::Box => {
                    painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, stroke, StrokeKind::Middle);
                }
                Tool::Ellipse => {
                    let area = Rect::from_two_pos(a, b);
                    painter.add(Shape::ellipse_stroke(
                        area.center(),
                        area.size() / 2.0,
                        stroke,
                    ));
                }
                Tool::Line => {
                    painter.line_segment([a, b], stroke);
                }
                Tool::Arrow => {
                    painter.arrow(a, b - a, stroke);
                }
                _ => {}
            }
        }
        if self.tool.draws_path() && !self.path.is_empty() {
            let mut points: Vec<Pos2> = self.path.iter().copied().map(to_screen).collect();
            if let Some(hover) = hover {
                let (at, snaps) = self.snap_point(scene, hover, t);
                points.push(to_screen(at));
                guides = snaps;
            }
            if self.tool == Tool::Polygon {
                painter.circle_stroke(points[0], tool::CLOSE, stroke);
                painter.add(Shape::closed_line(points, stroke));
            } else {
                painter.add(Shape::line(points, stroke));
            }
        }
        guides
    }

    /// Where an end of a line at canvas point `p` goes at time `t`: on
    /// the anchor there, or at `p` snapped as [`View::snap_point`] snaps
    /// it, with the guides of its snaps.
    fn end_point(&self, scene: &Scene, p: Point, t: f64) -> (Point, Option<Anchor>, Vec<Guide>) {
        match anchor::anchor_at(&self.outlines, p, self.anchor_reach()) {
            Some(anchor) => (anchor.at, Some(anchor), Vec::new()),
            None => {
                let (at, guides) = self.snap_point(scene, p, t);
                (at, None, guides)
            }
        }
    }

    /// How far, in canvas pixels, the pointer reaches to an anchor.
    pub fn anchor_reach(&self) -> f64 {
        (anchor::REACH / self.zoom) as f64
    }

    /// Draw the dots of the thing `anchor` is on, and mark the spot it
    /// takes, which may lie between the dots.
    fn draw_anchor(&self, painter: &egui::Painter, rect: Rect, anchor: &Anchor) {
        let blue = Color32::from_rgb(0x7a, 0xa2, 0xf7);
        for (_, at) in anchor.outline.dots() {
            let at = self.to_screen(rect, at);
            painter.circle_filled(at, 3.0, Color32::WHITE);
            painter.circle_stroke(at, 3.0, Stroke::new(1.0, blue));
        }
        painter.circle_filled(self.to_screen(rect, anchor.at), 4.5, blue);
    }

    /// Draw the outline of `target` and a square on each of its handles.
    fn draw_handles(&self, painter: &egui::Painter, rect: Rect, target: &Target) {
        let blue = Color32::from_rgb(0x7a, 0xa2, 0xf7);
        let stroke = Stroke::new(1.5, blue);
        let to_screen = |p: Point| self.to_screen(rect, p);
        match target {
            Target::Box { bounds, oval, .. } => {
                let area = Rect::from_min_max(to_screen(bounds.0), to_screen(bounds.1));
                if *oval {
                    painter.add(Shape::ellipse_stroke(
                        area.center(),
                        area.size() / 2.0,
                        stroke,
                    ));
                }
                painter.rect_stroke(area, 0.0, stroke, StrokeKind::Middle);
            }
            Target::Path { points, open, .. } => {
                let points: Vec<Pos2> = points.iter().copied().map(to_screen).collect();
                painter.add(if *open {
                    Shape::line(points, stroke)
                } else {
                    Shape::closed_line(points, stroke)
                });
            }
            Target::Line { ends, .. } => {
                painter.line_segment(ends.map(to_screen), stroke);
            }
        }
        for (_, at) in target.handles() {
            let square = Rect::from_center_size(to_screen(at), Vec2::splat(2.0 * handle::HANDLE));
            painter.rect_filled(square, 1.0, Color32::WHITE);
            painter.rect_stroke(square, 1.0, Stroke::new(1.0, blue), StrokeKind::Middle);
        }
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
    fn draw(&mut self, ctx: &egui::Context, scene: &Scene, wanted: &Drawn) -> bool {
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
        self.drawn = Some(wanted.clone());
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

/// The drag a press at screen point `at` starts with `tool`: on the
/// handle `grip`, on an object whose drag moves `nodes`, or on empty
/// canvas when `nodes` is empty. While `locked`, a press on an object or
/// a handle only clicks, and one on empty canvas draws a selection box.
fn press(
    tool: Tool,
    locked: bool,
    grip: Option<(&Target, Grip)>,
    nodes: Vec<Span>,
    at: Pos2,
) -> Drag {
    if locked {
        return match (grip, nodes.is_empty()) {
            (None, true) => Drag::new(nodes, at),
            _ => Drag::click(at),
        };
    }
    match (tool, grip) {
        (Tool::Select, Some((target, grip))) => Drag::grip(target.clone(), grip, at),
        (Tool::Select, None) => Drag::new(nodes, at),
        _ => Drag::create(at),
    }
}

/// The node that drew the object of `scene` at canvas point `p` at time
/// `t`, when a node did.
fn node_at(scene: &Scene, p: Point, t: f64) -> Option<Span> {
    scene
        .object_at(p, t)
        .map(|i| scene.objects[i].source)
        .filter(|s| s.start < s.end)
}

/// The indices of the objects in `scene` that the nodes at `nodes` or
/// nodes inside them drew.
fn drawn_by(scene: &Scene, nodes: &[Span]) -> Vec<usize> {
    let inside = |s: Span| nodes.iter().any(|n| n.start <= s.start && s.end <= n.end);
    (0..scene.objects.len())
        .filter(|&i| inside(scene.objects[i].source))
        .collect()
}

/// Where the nodes at `nodes`, dragged `by` canvas pixels, go at time
/// `t`: the box around their objects snapped to the boxes of the other objects and to
/// the grid of `grid` pixels within `reach` canvas pixels when `on`, and
/// moved by whole pixels when not.
fn snap_drag(
    scene: &Scene,
    nodes: &[Span],
    by: Point,
    t: f64,
    on: bool,
    reach: f64,
    grid: Option<f64>,
) -> Snapped {
    let lifted = drawn_by(scene, nodes);
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

/// The boxes of the objects of `scene` at time `t` that the node at
/// `node` did not draw.
fn others(scene: &Scene, node: Span, t: f64) -> Vec<Bounds> {
    let own = drawn_by(scene, &[node]);
    (0..scene.objects.len())
        .filter(|i| own.binary_search(i).is_err())
        .filter_map(|i| scene.bounds(i, t))
        .collect()
}

/// `target` as a drag of its handle `grip` by `by` canvas pixels leaves
/// it.
fn reshaped(target: &Target, grip: Grip, by: Point) -> Option<Target> {
    let mut out = target.clone();
    match (&mut out, target.reshape(grip, by)?) {
        (Target::Box { bounds, .. }, Reshape::Resize { to, .. }) => *bounds = to,
        (Target::Path { points, .. }, Reshape::Vertex { index, by, .. }) => {
            points[index] = points[index] + by
        }
        (Target::Line { ends, .. }, Reshape::End { end, to, .. }) => ends[end] = to,
        _ => return None,
    }
    Some(out)
}

/// The pointer's look over the handle `grip`.
fn cursor(grip: Grip) -> CursorIcon {
    match grip {
        Grip::Box([Edge::Middle, _]) => CursorIcon::ResizeVertical,
        Grip::Box([_, Edge::Middle]) => CursorIcon::ResizeHorizontal,
        Grip::Box([x, y]) if x == y => CursorIcon::ResizeNwSe,
        Grip::Box(_) => CursorIcon::ResizeNeSw,
        Grip::Vertex(_) | Grip::End(_) => CursorIcon::Move,
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
        let snapped = snap_drag(&scene, &[node], by, 0.0, true, 6.0, None);
        let edits =
            acre::scene::edit::move_edits(src, &[node], snapped.by.x, snapped.by.y).unwrap();
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
    fn several_nodes_snap_on_the_box_around_them_all() {
        // a and b span 0 to 50, whose centre at 25 + 84.5 comes within
        // 0.5 of c's centre at 110; no edge of a or b alone is within 2
        // of one of c's
        let src = "canvas { size 300 200; box a { at 0 0; size 20 10; } \
                   box b { at 30 0; size 20 10; } box c { at 100 50; size 20 20; } }";
        let scene = acre::scene::build(&acre::lang::parse(src).unwrap(), &[], &[]).unwrap();
        let nodes = [scene.objects[0].source, scene.objects[1].source];
        let snapped = snap_drag(&scene, &nodes, Point::new(84.5, 0.0), 0.0, true, 2.0, None);
        assert_eq!(snapped.by, Point::new(85.0, 0.0));
        let edits = acre::scene::edit::move_edits(src, &nodes, 85.0, 0.0).unwrap();
        let out = acre::scene::edit::apply(src, &edits);
        assert!(
            out.contains("at 85 0;") && out.contains("at 115 0;"),
            "{out}"
        );
    }

    #[test]
    fn an_unsnapped_drag_moves_by_whole_pixels() {
        let src =
            "canvas { size 300 200; box { at 0 0; size 20 10; } box { at 200 150; size 20 10; } }";
        let out = dragged(src, Point::new(41.6, 20.4));
        assert!(out.contains("at 42 20;"), "{out}");
    }

    fn press_on(tool: Tool, locked: bool, grip: bool, nodes: Vec<Span>) -> Release {
        let target = Target::Path {
            node: Span { start: 0, end: 9 },
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            open: true,
        };
        let grip = grip.then_some((&target, Grip::Vertex(1)));
        let mut drag = press(tool, locked, grip, nodes, pos2(0.0, 0.0));
        drag.follow(pos2(40.0, 30.0), 1.0);
        drag.release()
    }

    #[test]
    fn a_locked_press_on_an_object_or_a_handle_only_clicks() {
        let node = Span { start: 0, end: 9 };
        let click = Release::Click(pos2(0.0, 0.0));
        assert_eq!(press_on(Tool::Select, true, false, vec![node]), click);
        assert_eq!(press_on(Tool::Select, true, true, vec![node]), click);
        assert_eq!(press_on(Tool::Box, true, false, vec![node]), click);
        assert!(matches!(
            press_on(Tool::Select, false, false, vec![node]),
            Release::Move { .. }
        ));
        assert!(matches!(
            press_on(Tool::Select, false, true, vec![node]),
            Release::Grip { .. }
        ));
    }

    #[test]
    fn a_locked_press_on_empty_canvas_still_draws_a_selection_box() {
        let boxed = Release::Select(pos2(0.0, 0.0), pos2(40.0, 30.0));
        assert_eq!(press_on(Tool::Select, true, false, Vec::new()), boxed);
        assert_eq!(press_on(Tool::Box, true, false, Vec::new()), boxed);
        assert!(matches!(
            press_on(Tool::Box, false, false, Vec::new()),
            Release::Create(..)
        ));
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
