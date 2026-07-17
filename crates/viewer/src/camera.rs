//! Pan/zoom camera producing a 2D view transform. Pure math, no WebGL — unit
//! tested on the host target. Maps image pixel space to clip space `[-1, 1]`.

/// A 2D camera over an `nx × ny` image. `center` is the image-space point at the
/// canvas center; `zoom` is canvas-pixels per image-pixel.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub nx: f64,
    pub ny: f64,
    pub center: [f64; 2],
    pub zoom: f64,
}

impl Camera {
    #[must_use]
    pub fn new(nx: usize, ny: usize) -> Self {
        Self {
            nx: nx as f64,
            ny: ny as f64,
            center: [nx as f64 / 2.0, ny as f64 / 2.0],
            zoom: 1.0,
        }
    }

    /// Fit the whole image to a `canvas_w × canvas_h` viewport, centered.
    pub fn fit(&mut self, canvas_w: f64, canvas_h: f64) {
        self.center = [self.nx / 2.0, self.ny / 2.0];
        self.zoom = (canvas_w / self.nx).min(canvas_h / self.ny);
    }

    /// Column-major 3×3 (as 9 floats) mapping image space → clip space, given
    /// the canvas size. Image `y` is flipped so row 0 is at the top.
    #[must_use]
    pub fn matrix(&self, canvas_w: f64, canvas_h: f64) -> [f32; 9] {
        // clip.x = (img.x - cx) * zoom * 2/canvas_w
        // clip.y = (img.y - cy) * zoom * -2/canvas_h   (flip y)
        let sx = self.zoom * 2.0 / canvas_w;
        let sy = self.zoom * 2.0 / canvas_h;
        let (cx, cy) = (self.center[0], self.center[1]);
        [
            sx as f32,
            0.0,
            0.0,
            0.0,
            (-sy) as f32,
            0.0,
            (-cx * sx) as f32,
            (cy * sy) as f32,
            1.0,
        ]
    }

    /// Pan by a canvas-pixel delta (e.g. a mouse drag), in image-space units.
    pub fn pan(&mut self, dx_canvas: f64, dy_canvas: f64) {
        self.center[0] -= dx_canvas / self.zoom;
        self.center[1] += dy_canvas / self.zoom; // canvas y-down vs image y-up
    }

    /// Zoom by `factor` about a canvas point, keeping that point fixed under the
    /// cursor. `factor > 1` zooms in.
    pub fn zoom_about(&mut self, factor: f64, canvas_x: f64, canvas_y: f64, cw: f64, ch: f64) {
        let before = self.canvas_to_image(canvas_x, canvas_y, cw, ch);
        self.zoom = (self.zoom * factor).clamp(0.01, 1000.0);
        let after = self.canvas_to_image(canvas_x, canvas_y, cw, ch);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Canvas pixel → image pixel coordinate.
    #[must_use]
    pub fn canvas_to_image(&self, canvas_x: f64, canvas_y: f64, cw: f64, ch: f64) -> [f64; 2] {
        [
            self.center[0] + (canvas_x - cw / 2.0) / self.zoom,
            self.center[1] - (canvas_y - ch / 2.0) / self.zoom,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centers_and_scales() {
        let mut c = Camera::new(100, 50);
        c.fit(400.0, 400.0);
        assert!((c.center[0] - 50.0).abs() < 1e-9);
        assert!((c.center[1] - 25.0).abs() < 1e-9);
        assert!((c.zoom - 4.0).abs() < 1e-9); // 400/100 limits over 400/50
    }

    #[test]
    fn zoom_about_keeps_cursor_point_fixed() {
        let mut c = Camera::new(100, 100);
        c.fit(200.0, 200.0);
        let (px, py) = (150.0, 60.0);
        let before = c.canvas_to_image(px, py, 200.0, 200.0);
        c.zoom_about(2.0, px, py, 200.0, 200.0);
        let after = c.canvas_to_image(px, py, 200.0, 200.0);
        assert!(
            (before[0] - after[0]).abs() < 1e-9,
            "x drift {before:?} {after:?}"
        );
        assert!((before[1] - after[1]).abs() < 1e-9, "y drift");
    }

    #[test]
    fn canvas_center_maps_to_image_center() {
        let mut c = Camera::new(80, 60);
        c.fit(160.0, 160.0);
        let img = c.canvas_to_image(80.0, 80.0, 160.0, 160.0);
        assert!((img[0] - 40.0).abs() < 1e-9);
        assert!((img[1] - 30.0).abs() < 1e-9);
    }
}
