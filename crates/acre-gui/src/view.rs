use acre::render;
use acre::scene::{Point, Scene};
use eframe::egui::{
    self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, pos2,
};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 40.0;

/// The fraction of the pane a fitted canvas fills.
const FIT_MARGIN: f32 = 0.96;

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
}

/// What the person did in the pane this frame.
pub struct Shown {
    pub pane: Vec2,
    /// The canvas point under the pointer.
    pub hover: Option<Point>,
    /// The canvas point clicked.
    pub clicked: Option<Point>,
}

#[derive(PartialEq)]
struct Drawn {
    pixels: [usize; 2],
    zoom: f32,
    origin: Vec2,
    scene: u64,
}

impl View {
    pub fn new() -> Self {
        View {
            fit: true,
            zoom: 1.0,
            origin: Vec2::ZERO,
            texture: None,
            drawn: None,
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
        let pixels = [
            (rect.width() * ppp).round() as usize,
            (rect.height() * ppp).round() as usize,
        ];
        if pixels[0] == 0 || pixels[1] == 0 {
            return shown;
        }
        let drawn = Drawn {
            pixels,
            zoom: self.zoom,
            origin: self.origin,
            scene: version,
        };
        if self.drawn.as_ref() != Some(&drawn) {
            let pixmap = render::draw_view(
                scene,
                0.0,
                pixels[0] as u32,
                pixels[1] as u32,
                (self.zoom * ppp) as f64,
                Point::new((self.origin.x * ppp) as f64, (self.origin.y * ppp) as f64),
            );
            let image = egui::ColorImage::from_rgba_premultiplied(pixels, pixmap.data());
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "render",
                        image,
                        TextureOptions::NEAREST,
                    ));
                }
            }
            self.drawn = Some(drawn);
        }
        let painter = ui.painter_at(rect);
        if let Some(texture) = &self.texture {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
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
}
