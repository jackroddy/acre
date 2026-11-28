use acre::lang::Span;
use acre::render;
use acre::scene::edit::Siblings;
use acre::scene::{Animation, Point, Scene, Track};
use tiny_skia as sk;
use una::{Buffer, Button, Event, Format, MenuEntry, Palette, Rgba, Surface, Ui};

use crate::anchor::{self, Anchor, Outline};
use crate::arrange;
use crate::drag::{Drag, Release};
use crate::handle::{self, Grip, Reshape, Target};
use crate::marks::{self, Mark};
use crate::reorder;
use crate::screen;
use crate::selection;
use crate::snap::{self, Bounds, Guide, Snapped};
use crate::tool::{self, Tool};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 40.0;

/// The fraction of the pane a fitted canvas fills.
const FIT_MARGIN: f32 = 0.96;

/// How long the view waits after the last pan or zoom before it draws
/// the scene again, in milliseconds.
const SETTLE: u64 = 150;

/// How much one notch of the wheel zooms.
const ZOOM_STEP: f32 = 1.25;

const GRID: Rgba = [0.5, 0.5, 0.5, 0.31];
const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];

/// The render pane: the scene drawn at the pane's own pixels, zoomed and
/// panned by the person.
pub struct View {
    /// Whether the canvas follows the pane's size, until the person
    /// zooms or pans.
    fit: bool,
    /// Pixels on screen per canvas pixel.
    zoom: f32,
    /// Where the canvas's top-left corner sits, from the pane's.
    origin: screen::Point,
    /// The pane's size at the last frame.
    pane: screen::Point,
    /// Pixels on screen per point of the sizes of handles, outlines and
    /// reaches, which grows with the window's text.
    unit: f32,
    /// What `base` shows.
    drawn: Option<Drawn>,
    /// The scene drawn for `drawn`, on the window's background.
    base: Option<sk::Pixmap>,
    /// The marks drawn over `base` in the buffer presented last.
    marks: Vec<Mark>,
    /// The size of the buffer of background alone presented while there
    /// is no scene to draw.
    cleared: Option<(u32, u32)>,
    /// When the person last panned or zoomed, in the milliseconds of
    /// `Ui::now_ms`.
    moved: Option<u64>,
    /// Where the pointer was in the window while the middle button
    /// pans, until it is let go.
    panning: Option<(f64, f64)>,
    /// The primary press in the pane, until it is let go.
    drag: Option<Drag>,
    /// The nodes let go of after a drag, and how far, drawn there until
    /// the source they were moved in comes back.
    held: Option<(Vec<Span>, Point)>,
    /// The points placed so far of the polygon or polyline being drawn,
    /// in canvas pixels.
    pub path: Vec<Point>,
    /// The one selected node that shows handles.
    pub target: Option<Target>,
    /// The things an end of a line can go on, while one is drawn or
    /// moved.
    pub outlines: Vec<Outline>,
}

/// The settings the toolbar sets, which every drawing shares.
pub struct Toolbar {
    /// Whether dragged nodes snap to other objects and the grid.
    pub snap: bool,
    /// Whether the grid is drawn and snapped to.
    pub grid: bool,
    /// The grid's spacing in canvas pixels.
    pub spacing: f64,
    /// Whether the view refuses every edit to the source.
    pub locked: bool,
    pub tool: Tool,
}

impl Toolbar {
    pub fn new() -> Self {
        Toolbar {
            snap: true,
            grid: false,
            spacing: 10.0,
            locked: false,
            tool: Tool::Select,
        }
    }
}

/// What the person did in the pane this frame.
pub struct Shown {
    /// Where the pane is in the window.
    pub rect: screen::Rect,
    /// Whether the left button went down on the pane this frame.
    pub pane_pressed: bool,
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

const MENU: [MenuItem; 2] = [MenuItem::Delete, MenuItem::Duplicate];

/// The view the base was drawn for.
#[derive(Clone, PartialEq)]
struct Drawn {
    /// The pane's size in pixels.
    pane: (u32, u32),
    zoom: f32,
    origin: screen::Point,
    scene: u64,
    time: f64,
    /// The nodes drawn moved by a drag, and how far.
    preview: Option<(Vec<Span>, Point)>,
    background: Rgba,
}

impl View {
    pub fn new() -> Self {
        View {
            fit: true,
            zoom: 1.0,
            origin: screen::Point::ZERO,
            pane: screen::Point::ZERO,
            unit: 1.0,
            drawn: None,
            base: None,
            marks: Vec::new(),
            cleared: None,
            moved: None,
            panning: None,
            drag: None,
            held: None,
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

    /// Draw the scene again on the next frame, as the pane has shown
    /// another drawing since.
    pub fn forget(&mut self) {
        self.drawn = None;
        self.base = None;
        self.cleared = None;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn fit(&mut self) {
        self.fit = true;
    }

    /// Zoom by `factor` about the middle of the pane.
    pub fn zoom_by(&mut self, factor: f32, now: u64) {
        self.zoom_about(factor, self.pane / 2.0, now);
    }

    /// Show the canvas at one pixel on screen to a canvas pixel.
    pub fn actual_size(&mut self, now: u64) {
        self.zoom_about(1.0 / self.zoom, self.pane / 2.0, now);
    }

    fn zoom_about(&mut self, factor: f32, at: screen::Point, now: u64) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let factor = zoom / self.zoom;
        self.origin = at - (at - self.origin) * factor;
        self.zoom = zoom;
        self.fit = false;
        self.moved = Some(now);
    }

    /// How far `points` reach on the canvas, in canvas pixels.
    pub fn reach(&self, points: f32) -> f64 {
        (points * self.unit / self.zoom) as f64
    }

    /// How far, in canvas pixels, the pointer reaches to an anchor.
    pub fn anchor_reach(&self) -> f64 {
        self.reach(anchor::REACH)
    }

    /// Lay the pane out in `ui` as `surface`, take the person's zoom, pan
    /// and drag with the toolbar's settings `bar`, and draw `scene`, whose
    /// version is `version`, at time `t`, with an outline around each
    /// object the nodes at `selected` draw. `unit` is the pixels on screen
    /// to a point of the marks drawn over the scene.
    ///
    /// While nodes are dragged, their objects are drawn where they are
    /// dragged to, snapped, and outlined, with a guide for each snap. A
    /// drag that reorders a child of a stack or grid outlines it at the
    /// pointer and marks the place it would land. A drag on empty canvas
    /// draws a selection box. A double-click on no object fits the canvas
    /// to the pane. With a tool other than Select, a press draws a new
    /// node, outlined as it goes. A right-click on an object opens its
    /// menu. With the Select tool, the node in `target` shows handles,
    /// and a drag on one draws the outline it makes, snapped. With the
    /// Line and Arrow tools, and while the end of a line is dragged, the
    /// thing under the pointer shows the dots an end snaps to.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        surface: &mut Surface,
        unit: f32,
        bar: &Toolbar,
        scene: Option<&mut Scene>,
        version: u64,
        t: f64,
        selected: &[Span],
    ) -> Shown {
        let pane = ui.surface(surface);
        let signal = pane.signal;
        let now = ui.now_ms();
        self.unit = unit;
        self.pane = screen::Point::new(pane.size.0 as f32, pane.size.1 as f32);
        let mut shown = Shown {
            rect: screen::Rect {
                min: screen::Point::new(signal.rect.x as f32, signal.rect.y as f32),
                size: self.pane,
            },
            pane_pressed: signal.pressed,
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
        self.pan(ui, signal.rect, now);
        let menu = ui.id("object menu");
        let entries = MENU.map(|item| MenuEntry {
            available: !bar.locked,
            ..MenuEntry::new(match item {
                MenuItem::Delete => "Delete",
                MenuItem::Duplicate => "Duplicate",
            })
        });
        let Some(scene) = scene else {
            self.drag = None;
            shown.chosen = ui.menu(menu, &entries).ran.map(|i| MENU[i]);
            self.clear(surface, pane.size, ui.palette().background);
            return shown;
        };
        let canvas = screen::Point::new(scene.canvas.width as f32, scene.canvas.height as f32);
        let target = self.target.clone().filter(|_| bar.tool == Tool::Select);
        let target = target.as_ref();
        let handle_reach = self.reach(handle::HANDLE + 2.0);
        let pointer = signal
            .at
            .map(|(x, y)| screen::Point::new(x as f32, y as f32));
        let hover = pointer.filter(|_| signal.hovered);

        // before this frame's pan and zoom, so the press lands on what
        // the last frame showed
        if signal.pressed
            && let Some(at) = pointer
        {
            let p = self.to_canvas(at);
            let grip = target.and_then(|target| Some((target, target.grip_at(p, handle_reach)?)));
            let nodes =
                node_at(scene, p, t).map_or(Vec::new(), |node| selection::dragged(selected, node));
            if let [node] = nodes[..]
                && bar.tool == Tool::Select
                && grip.is_none()
                && !bar.locked
            {
                shown.pressed = Some(node);
            }
            self.drag = Some(press(bar.tool, bar.locked, grip, nodes, at));
        }
        if signal.right_clicked
            && let Some(at) = pointer
        {
            shown.right_clicked = node_at(scene, self.to_canvas(at), t);
            if shown.right_clicked.is_some() {
                let at = una::Point {
                    x: signal.rect.x + at.x as i32,
                    y: signal.rect.y + at.y as i32,
                };
                ui.open_menu(menu, at);
            }
        }
        shown.chosen = ui.menu(menu, &entries).ran.map(|i| MENU[i]);
        if signal.double_clicked && bar.tool == Tool::Select {
            match pointer.and_then(|at| node_at(scene, self.to_canvas(at), t)) {
                Some(node) => shown.double_clicked = Some(node),
                None => self.fit = true,
            }
        }
        if signal.scrolled.1 != 0.0
            && let Some(at) = hover
        {
            self.zoom_about(ZOOM_STEP.powf(-signal.scrolled.1 as f32), at, now);
        }
        if self.fit {
            self.zoom = (self.pane.x / canvas.x)
                .min(self.pane.y / canvas.y)
                .clamp(MIN_ZOOM, MAX_ZOOM)
                * FIT_MARGIN;
            self.origin = (self.pane - canvas * self.zoom) / 2.0;
        }

        if let Some(drag) = &mut self.drag
            && let Some(at) = pointer
        {
            drag.follow(at, self.zoom);
        }
        let mut guides = Vec::new();
        let snap_reach = self.reach(snap::REACH);
        if let Some(drag) = &mut self.drag
            && let Some((nodes, by)) = drag.moving()
        {
            let grid = bar.grid.then_some(bar.spacing);
            let snapped = snap_drag(scene, nodes, by, t, bar.snap, snap_reach, grid);
            drag.place(snapped.by);
            guides = snapped.guides;
        }
        let place = pointer.and_then(|at| self.aim_reorder(scene, t, self.to_canvas(at)));
        if let Some(drag) = &mut self.drag
            && let Some((target, grip, by)) = drag.gripping()
            && !matches!(grip, Grip::End(_))
            && let Some(at) = target.grip_point(grip)
        {
            let others: Vec<Bounds> = others(scene, target.node(), t);
            let grid = bar.grid.then_some(bar.spacing);
            let axes = handle::axes(grip);
            let (to, snaps) =
                handle::snap_handle(at + by, axes, &others, snap_reach, grid, bar.snap);
            drag.place(to - at);
            guides = snaps;
        }
        if !signal.held
            && let Some(drag) = self.drag.take()
        {
            self.release(drag, &mut shown);
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

        let palette = ui.palette().clone();
        let hover_canvas = hover.map(|p| self.to_canvas(p));
        let mut marks = self.marks_under(&palette, bar, scene, t, selected, &lifted, place);
        guides.extend(self.draw_preview(&palette, bar, &mut marks, scene, t, hover_canvas));
        if let Some(target) = target {
            self.draw_target(&palette, bar, &mut marks, &mut guides, scene, t, target);
        }
        for g in &guides {
            marks.push(Mark::Line {
                from: self.to_screen(g.from),
                to: self.to_screen(g.to),
                width: unit,
                color: palette.red,
            });
        }
        let wanted = Drawn {
            pane: pane.size,
            zoom: self.zoom,
            origin: self.origin,
            scene: version,
            time: t,
            preview,
            background: palette.background,
        };
        self.present(ui, surface, scene, wanted, marks);

        for &i in &lifted {
            scene.objects[i].groups.pop();
        }
        shown.hover = hover_canvas;
        shown
    }

    /// Pan by what the pointer travels while the middle button is down,
    /// pressed in the pane at `pane` in the window.
    fn pan(&mut self, ui: &Ui, pane: una::Rect, now: u64) {
        for event in ui.events() {
            match *event {
                Event::PointerButton {
                    button: Button::Middle,
                    pressed,
                    at,
                    ..
                } => {
                    self.panning = (pressed && pane.contains(at.0, at.1)).then_some(at);
                }
                Event::PointerMoved { at } => {
                    if let Some(from) = self.panning {
                        let by = screen::Point::new((at.0 - from.0) as f32, (at.1 - from.1) as f32);
                        self.origin = self.origin + by;
                        self.fit = false;
                        self.moved = Some(now);
                        self.panning = Some(at);
                    }
                }
                _ => {}
            }
        }
    }

    /// Note on `shown` what `drag`, let go of this frame, comes to.
    fn release(&mut self, drag: Drag, shown: &mut Shown) {
        match drag.release() {
            Release::Click(at) => shown.clicked = Some(self.to_canvas(at)),
            Release::Move { nodes, by } => {
                self.held = Some((nodes.clone(), by));
                shown.moved = Some((nodes, by));
            }
            Release::Select(from, to) => {
                let (a, b) = (self.to_canvas(from), self.to_canvas(to));
                shown.boxed = Some((
                    Point::new(a.x.min(b.x), a.y.min(b.y)),
                    Point::new(a.x.max(b.x), a.y.max(b.y)),
                ));
            }
            Release::Create(from, to) => {
                shown.created = Some((self.to_canvas(from), self.to_canvas(to)));
            }
            Release::Grip { target, grip, by } => shown.reshaped = target.reshape(grip, by),
            Release::Reorder {
                node,
                before,
                index,
            } => shown.reordered = Some((node, before, index)),
        }
    }

    /// The marks under the previews and handles: the grid, the outlines
    /// of the objects lifted by a drag and of the selection, and the
    /// place and outline of a reorder.
    #[allow(clippy::too_many_arguments)]
    fn marks_under(
        &self,
        palette: &Palette,
        bar: &Toolbar,
        scene: &Scene,
        t: f64,
        selected: &[Span],
        lifted: &[usize],
        place: Option<[Point; 2]>,
    ) -> Vec<Mark> {
        let unit = self.unit;
        let mut marks = Vec::new();
        if bar.grid {
            let step = snap::grid_step(bar.spacing, self.zoom / unit);
            let (lo, hi) = (
                self.to_canvas(screen::Point::ZERO),
                self.to_canvas(self.pane),
            );
            let (width, height) = (scene.canvas.width as f64, scene.canvas.height as f64);
            let (top, bottom) = (lo.y.max(0.0), hi.y.min(height));
            let (left, right) = (lo.x.max(0.0), hi.x.min(width));
            let line = |from: Point, to: Point| Mark::Line {
                from: self.to_screen(from),
                to: self.to_screen(to),
                width: 1.0,
                color: GRID,
            };
            for x in multiples(left, right, step) {
                marks.push(line(Point::new(x, top), Point::new(x, bottom)));
            }
            for y in multiples(top, bottom, step) {
                marks.push(line(Point::new(left, y), Point::new(right, y)));
            }
        }
        let outline = |min: Point, max: Point, gap: f32, color: Rgba| {
            let width = 2.0 * unit;
            let rect = screen::Rect::from_corners(self.to_screen(min), self.to_screen(max));
            Mark::Frame {
                rect: rect.expand(gap + width / 2.0),
                width,
                color,
            }
        };
        for (min, max) in lifted.iter().filter_map(|&i| scene.bounds(i, t)) {
            marks.push(outline(min, max, 3.0 * unit, palette.yellow));
        }

        // after the lift, so a dragged selection is outlined where it is
        // drawn, and wider than a mark, so both show on one object
        for (min, max) in drawn_by(scene, selected)
            .into_iter()
            .filter_map(|i| scene.bounds(i, t))
        {
            marks.push(outline(min, max, 7.0 * unit, palette.accent));
        }
        if let Some([from, to]) = place {
            marks.push(Mark::Line {
                from: self.to_screen(from),
                to: self.to_screen(to),
                width: 3.0 * unit,
                color: palette.yellow,
            });
        }
        if let Some((node, _, by)) = self.drag.as_ref().and_then(Drag::reordering)
            && let Some((min, max)) = arrange::bounds(scene, node, t)
        {
            marks.push(outline(min + by, max + by, 0.0, palette.accent));
        }
        if let Some((from, to)) = self.drag.as_ref().and_then(Drag::boxed) {
            let rect = screen::Rect::from_corners(from, to);
            let [red, green, blue, _] = palette.accent;
            marks.push(Mark::Fill {
                rect,
                color: [red, green, blue, 0.15],
            });
            marks.push(Mark::Frame {
                rect: rect.expand(-0.5),
                width: 1.0,
                color: palette.accent,
            });
        }
        marks
    }

    /// Add the handles of `target` to `marks`, with the outline a drag of
    /// one makes and the guides of its snaps.
    #[allow(clippy::too_many_arguments)]
    fn draw_target(
        &self,
        palette: &Palette,
        bar: &Toolbar,
        marks: &mut Vec<Mark>,
        guides: &mut Vec<Guide>,
        scene: &Scene,
        t: f64,
        target: &Target,
    ) {
        let gripped = self.drag.as_ref().and_then(Drag::gripping);
        let dragged = gripped.and_then(|(gripped, grip, by)| {
            let Grip::End(_) = grip else {
                return reshaped(gripped, grip, by);
            };
            let from = gripped.grip_point(grip)?;
            let (to, anchor, snaps) = self.end_point(bar, scene, from + by, t);
            if let Some(anchor) = anchor {
                self.draw_anchor(palette, marks, &anchor);
            }
            guides.extend(snaps);
            reshaped(gripped, grip, to - from)
        });
        self.draw_handles(palette, marks, dragged.as_ref().unwrap_or(target));
    }

    /// Present `marks` over the scene drawn for `wanted` on `surface`.
    /// Between pans and zooms the last buffer is moved and stretched into
    /// place, and the scene is drawn again only once the person stops.
    fn present(
        &mut self,
        ui: &mut Ui,
        surface: &mut Surface,
        scene: &Scene,
        wanted: Drawn,
        marks: Vec<Mark>,
    ) {
        let (width, height) = wanted.pane;
        if width == 0 || height == 0 {
            return;
        }
        let now = ui.now_ms();
        let settles = self.moved.map_or(0, |moved| moved + SETTLE);
        let settled = now >= settles && self.panning.is_none();
        let stale = match &self.drawn {
            None => true,
            Some(d) => {
                d.scene != wanted.scene
                    || d.time != wanted.time
                    || d.pane != wanted.pane
                    || d.preview != wanted.preview
                    || d.background != wanted.background
                    || (*d != wanted && settled)
            }
        };
        if stale {
            self.base = Some(draw(scene, &wanted));
            self.drawn = Some(wanted);
        }
        let (Some(drawn), Some(base)) = (&self.drawn, &self.base) else {
            return;
        };
        if drawn.zoom != self.zoom || drawn.origin != self.origin {
            let [x, y, w, h] = stretched(drawn, self.zoom, self.origin);
            surface.view(x, y, w, h);
            if self.panning.is_none() {
                ui.wake_at(settles);
            }
            return;
        }
        if stale || marks != self.marks {
            let mut buffer = Buffer::new(width, height, Format::Rgba);
            buffer.pixels().copy_from_slice(base.data());
            if let Some(mut pixmap) = sk::PixmapMut::from_bytes(buffer.pixels(), width, height) {
                marks::paint(&mut pixmap, &marks);
            }
            surface.present(buffer);
            self.marks = marks;
            self.cleared = None;
        }
        surface.view(0.0, 0.0, width as f64, height as f64);
    }

    /// Present the background alone, once for each size of the pane, so
    /// the pane shows no other drawing's render.
    fn clear(&mut self, surface: &mut Surface, size: (u32, u32), background: Rgba) {
        let (width, height) = size;
        if self.cleared == Some(size) || width == 0 || height == 0 {
            return;
        }
        let mut buffer = Buffer::new(width, height, Format::Rgba);
        if let Some(mut pixmap) = sk::PixmapMut::from_bytes(buffer.pixels(), width, height) {
            let [red, green, blue, alpha] = background;
            pixmap.fill(sk::Color::from_rgba(red, green, blue, alpha).unwrap_or(sk::Color::BLACK));
        }
        surface.present(buffer);
        surface.view(0.0, 0.0, width as f64, height as f64);
        self.cleared = Some(size);
        self.drawn = None;
        self.base = None;
    }

    /// Canvas point `p` snapped to the edges and centres of the objects
    /// of `scene` at time `t` and to the grid, as a dragged object snaps,
    /// or rounded to whole pixels when `bar` turns snapping off, with a
    /// guide for each snap.
    pub fn snap_point(
        &self,
        bar: &Toolbar,
        scene: &Scene,
        p: Point,
        t: f64,
    ) -> (Point, Vec<Guide>) {
        let p = Point::new(p.x.round(), p.y.round());
        if !bar.snap {
            return (p, Vec::new());
        }
        let others: Vec<Bounds> = (0..scene.objects.len())
            .filter_map(|i| scene.bounds(i, t))
            .collect();
        let reach = self.reach(snap::REACH);
        let grid = bar.grid.then_some(bar.spacing);
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

    /// Add to `marks` the outline of the node the tool is drawing, with
    /// the pointer at canvas point `hover`, and give the guides of its
    /// snaps.
    fn draw_preview(
        &self,
        palette: &Palette,
        bar: &Toolbar,
        marks: &mut Vec<Mark>,
        scene: &Scene,
        t: f64,
        hover: Option<Point>,
    ) -> Vec<Guide> {
        let (width, color) = (1.5 * self.unit, palette.accent);
        let mut guides = Vec::new();
        let line = matches!(bar.tool, Tool::Line | Tool::Arrow);
        if line
            && self.drag.is_none()
            && let Some(hover) = hover
            && let (_, Some(anchor), _) = self.end_point(bar, scene, hover, t)
        {
            self.draw_anchor(palette, marks, &anchor);
        }
        if let Some((from, to)) = self.drag.as_ref().and_then(Drag::creating) {
            let (from, to) = (self.to_canvas(from), self.to_canvas(to));
            let (from, to) = if line {
                let (from, from_anchor, _) = self.end_point(bar, scene, from, t);
                let (to, to_anchor, snaps) = self.end_point(bar, scene, to, t);
                for anchor in [from_anchor, to_anchor].iter().flatten() {
                    self.draw_anchor(palette, marks, anchor);
                }
                guides = snaps;
                (from, to)
            } else {
                let (from, _) = self.snap_point(bar, scene, from, t);
                let (to, snaps) = self.snap_point(bar, scene, to, t);
                guides = snaps;
                (from, to)
            };
            let (from, to) = (self.to_screen(from), self.to_screen(to));
            let rect = screen::Rect::from_corners(from, to);
            match bar.tool {
                Tool::Box => marks.push(Mark::Frame { rect, width, color }),
                Tool::Ellipse => marks.push(Mark::Oval { rect, width, color }),
                Tool::Line => marks.push(Mark::Line {
                    from,
                    to,
                    width,
                    color,
                }),
                Tool::Arrow => marks.push(Mark::Arrow {
                    from,
                    to,
                    width,
                    color,
                }),
                _ => {}
            }
        }
        if bar.tool.draws_path() && !self.path.is_empty() {
            let mut points: Vec<screen::Point> =
                self.path.iter().map(|&p| self.to_screen(p)).collect();
            if let Some(hover) = hover {
                let (at, snaps) = self.snap_point(bar, scene, hover, t);
                points.push(self.to_screen(at));
                guides = snaps;
            }
            let closed = bar.tool == Tool::Polygon;
            if closed {
                marks.push(Mark::Ring {
                    at: points[0],
                    radius: tool::CLOSE * self.unit,
                    width,
                    color,
                });
            }
            marks.push(Mark::Path {
                points,
                closed,
                width,
                color,
            });
        }
        guides
    }

    /// Where an end of a line at canvas point `p` goes at time `t`: on
    /// the anchor there, or at `p` snapped as [`View::snap_point`] snaps
    /// it, with the guides of its snaps.
    fn end_point(
        &self,
        bar: &Toolbar,
        scene: &Scene,
        p: Point,
        t: f64,
    ) -> (Point, Option<Anchor>, Vec<Guide>) {
        match anchor::anchor_at(&self.outlines, p, self.anchor_reach()) {
            Some(anchor) => (anchor.at, Some(anchor), Vec::new()),
            None => {
                let (at, guides) = self.snap_point(bar, scene, p, t);
                (at, None, guides)
            }
        }
    }

    /// Add to `marks` the dots of the thing `anchor` is on, and a mark on
    /// the spot it takes, which may lie between the dots.
    fn draw_anchor(&self, palette: &Palette, marks: &mut Vec<Mark>, anchor: &Anchor) {
        let unit = self.unit;
        for (_, at) in anchor.outline.dots() {
            let at = self.to_screen(at);
            marks.push(Mark::Dot {
                at,
                radius: 3.0 * unit,
                color: WHITE,
            });
            marks.push(Mark::Ring {
                at,
                radius: 3.0 * unit,
                width: unit,
                color: palette.accent,
            });
        }
        marks.push(Mark::Dot {
            at: self.to_screen(anchor.at),
            radius: 4.5 * unit,
            color: palette.accent,
        });
    }

    /// Add to `marks` the outline of `target` and a square on each of its
    /// handles.
    fn draw_handles(&self, palette: &Palette, marks: &mut Vec<Mark>, target: &Target) {
        let (width, color) = (1.5 * self.unit, palette.accent);
        match target {
            Target::Box { bounds, oval, .. } => {
                let rect =
                    screen::Rect::from_corners(self.to_screen(bounds.0), self.to_screen(bounds.1));
                if *oval {
                    marks.push(Mark::Oval { rect, width, color });
                }
                marks.push(Mark::Frame { rect, width, color });
            }
            Target::Path { points, open, .. } => marks.push(Mark::Path {
                points: points.iter().map(|&p| self.to_screen(p)).collect(),
                closed: !*open,
                width,
                color,
            }),
            Target::Line { ends, .. } => marks.push(Mark::Line {
                from: self.to_screen(ends[0]),
                to: self.to_screen(ends[1]),
                width,
                color,
            }),
        }
        let side = handle::HANDLE * self.unit;
        for (_, at) in target.handles() {
            let at = self.to_screen(at);
            let rect = screen::Rect::from_corners(
                at - screen::Point::new(side, side),
                at + screen::Point::new(side, side),
            );
            marks.push(Mark::Fill { rect, color: WHITE });
            marks.push(Mark::Frame {
                rect,
                width: self.unit,
                color,
            });
        }
    }

    /// The point in the pane at canvas point `p`.
    pub fn to_screen(&self, p: Point) -> screen::Point {
        self.origin + screen::Point::new(p.x as f32, p.y as f32) * self.zoom
    }

    /// The canvas point at point `p` in the pane.
    fn to_canvas(&self, p: screen::Point) -> Point {
        let v = (p - self.origin) / self.zoom;
        Point::new(v.x as f64, v.y as f64)
    }
}

/// `scene` drawn for the view `wanted`, on its background.
fn draw(scene: &Scene, wanted: &Drawn) -> sk::Pixmap {
    let (width, height) = wanted.pane;
    let origin = Point::new(wanted.origin.x as f64, wanted.origin.y as f64);
    let drawn = render::draw_view(
        scene,
        wanted.time,
        width,
        height,
        wanted.zoom as f64,
        origin,
    );
    let mut base = sk::Pixmap::new(width, height).expect("the pane has a nonzero size");
    let [red, green, blue, alpha] = wanted.background;
    base.fill(sk::Color::from_rgba(red, green, blue, alpha).unwrap_or(sk::Color::BLACK));
    base.draw_pixmap(
        0,
        0,
        drawn.as_ref(),
        &sk::PixmapPaint::default(),
        sk::Transform::identity(),
        None,
    );
    base
}

/// The part of the buffer drawn for `drawn` that fills the pane at the
/// current `zoom` and `origin`, as x, y, width and height in its pixels.
fn stretched(drawn: &Drawn, zoom: f32, origin: screen::Point) -> [f64; 4] {
    // the canvas point at pane point s is (s - origin) / zoom, and the
    // buffer shows it at drawn.origin + (s - origin) / zoom * drawn.zoom
    let scale = (drawn.zoom / zoom) as f64;
    let x = drawn.origin.x as f64 - origin.x as f64 * scale;
    let y = drawn.origin.y as f64 - origin.y as f64 * scale;
    let (width, height) = drawn.pane;
    [x, y, width as f64 * scale, height as f64 * scale]
}

/// The drag a press at pane point `at` starts with `tool`: on the
/// handle `grip`, on an object whose drag moves `nodes`, or on empty
/// canvas when `nodes` is empty. While `locked`, a press on an object or
/// a handle only clicks, and one on empty canvas draws a selection box.
fn press(
    tool: Tool,
    locked: bool,
    grip: Option<(&Target, Grip)>,
    nodes: Vec<Span>,
    at: screen::Point,
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

/// The multiples of `step` from `lo` to `hi`.
fn multiples(lo: f64, hi: f64, step: f64) -> impl Iterator<Item = f64> {
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    (first..=last).map(move |k| k as f64 * step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(zoom: f32, origin: screen::Point) -> Drawn {
        Drawn {
            pane: (400, 200),
            zoom,
            origin,
            scene: 0,
            time: 0.0,
            preview: None,
            background: [0.0; 4],
        }
    }

    #[test]
    fn an_unmoved_buffer_fills_the_pane() {
        let origin = screen::Point::new(30.0, 40.0);
        let view = stretched(&drawn(2.0, origin), 2.0, origin);
        assert_eq!(view, [0.0, 0.0, 400.0, 200.0]);
    }

    #[test]
    fn a_stretched_buffer_follows_a_pan_and_scales_with_a_zoom() {
        let last = drawn(1.0, screen::Point::ZERO);

        // panned 25 pixels right, the pane shows the buffer from 25
        // pixels left of its edge
        let view = stretched(&last, 1.0, screen::Point::new(25.0, 0.0));
        assert_eq!(view, [-25.0, 0.0, 400.0, 200.0]);

        // zoomed 2x about the pane's top-left corner, the pane shows
        // the buffer's top-left quarter
        let mut view_of = View::new();
        view_of.zoom_about(2.0, screen::Point::ZERO, 0);
        let view = stretched(&last, view_of.zoom, view_of.origin);
        assert_eq!(view, [0.0, 0.0, 200.0, 100.0]);
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
        let mut drag = press(tool, locked, grip, nodes, screen::Point::ZERO);
        drag.follow(screen::Point::new(40.0, 30.0), 1.0);
        drag.release()
    }

    #[test]
    fn a_locked_press_on_an_object_or_a_handle_only_clicks() {
        let node = Span { start: 0, end: 9 };
        let click = Release::Click(screen::Point::ZERO);
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
        let boxed = Release::Select(screen::Point::ZERO, screen::Point::new(40.0, 30.0));
        assert_eq!(press_on(Tool::Select, true, false, Vec::new()), boxed);
        assert_eq!(press_on(Tool::Box, true, false, Vec::new()), boxed);
        assert!(matches!(
            press_on(Tool::Box, false, false, Vec::new()),
            Release::Create(..)
        ));
    }
}
