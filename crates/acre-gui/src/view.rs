use std::time::{Duration, Instant};

use acre::render;
use acre::scene::{Point, Scene};
use eframe::egui::{
    self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, pos2,
};

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
}

/// What the person did in the pane this frame.
pub struct Shown {
    pub pane: Vec2,
    /// The canvas point under the pointer.
    pub hover: Option<Point>,
    /// The canvas point clicked.
    pub clicked: Option<Point>,
}

/// The view the texture was drawn for.
#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    pane: Vec2,
    pixels_per_point: f32,
    zoom: f32,
    origin: Vec2,
    scene: u64,
}

impl Drawn {
    /// How far past each edge of the pane the texture reaches, in points.
    fn overdraw(&self) -> Vec2 {
        (self.pane * OVERDRAW).round()
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
        }
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

    /// Lay the pane out in `ui`, take the person's zoom and pan, and draw
    /// `scene`, whose version is `version`, with an outline around each
    /// box in `marks`, given in canvas pixels.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: Option<&Scene>,
        version: u64,
        marks: &[(Point, Point)],
    ) -> Shown {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let mut shown = Shown {
            pane: rect.size(),
            hover: None,
            clicked: None,
        };
        let Some(scene) = scene else {
            return shown;
        };
        let canvas = Vec2::new(scene.canvas.width as f32, scene.canvas.height as f32);

        if response.double_clicked() {
            self.fit = true;
        }
        if response.dragged() {
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

        let ppp = ui.ctx().pixels_per_point();
        let wanted = Drawn {
            pane: rect.size(),
            pixels_per_point: ppp,
            zoom: self.zoom,
            origin: self.origin,
            scene: version,
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
                    || d.pane != wanted.pane
                    || d.pixels_per_point != ppp
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
        let to_screen =
            |p: Point| rect.min + self.origin + Vec2::new(p.x as f32, p.y as f32) * self.zoom;
        for &(min, max) in marks {
            let mark = Rect::from_min_max(to_screen(min), to_screen(max)).expand(3.0);
            painter.rect_stroke(
                mark,
                3.0,
                Stroke::new(2.0, Color32::from_rgb(0xff, 0x9e, 0x64)),
                StrokeKind::Outside,
            );
        }

        let to_canvas = |p: Pos2| {
            let v = (p - rect.min - self.origin) / self.zoom;
            Point::new(v.x as f64, v.y as f64)
        };
        shown.hover = response.hover_pos().map(to_canvas);
        if response.clicked() {
            shown.clicked = response.interact_pointer_pos().map(to_canvas);
        }
        shown
    }

    /// Draw `scene` into the texture for the view `wanted`, with the
    /// overdraw around the pane. False when the pane has no pixels.
    fn draw(&mut self, ctx: &egui::Context, scene: &Scene, wanted: Drawn) -> bool {
        let overdraw = wanted.overdraw();
        let area = wanted.pane + overdraw * 2.0;
        let ppp = wanted.pixels_per_point;
        let pixels = [
            (area.x * ppp).round() as usize,
            (area.y * ppp).round() as usize,
        ];
        if pixels[0] == 0 || pixels[1] == 0 {
            return false;
        }
        let origin = (wanted.origin + overdraw) * ppp;
        let pixmap = render::draw_view(
            scene,
            0.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(zoom: f32, origin: Vec2) -> Drawn {
        Drawn {
            pane: Vec2::new(400.0, 200.0),
            pixels_per_point: 1.0,
            zoom,
            origin,
            scene: 0,
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
}
