//! Pan/zoom camera producing a 2D view transform. Pure math, no WebGL — unit
//! tested on the host target. Maps image pixel space to clip space `[-1, 1]`.

/// Minimum image span (in image pixels) visible along either axis at max zoom.
/// Below this, further zoom-in is refused — matches the Python viewer's
/// `min_span_pixels = 5` in `data/model/coordinates.py`.
pub const MIN_SPAN_PIXELS: f64 = 5.0;

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
        self.zoom = self.min_zoom(canvas_w, canvas_h);
    }

    /// Zoom-out floor: whole image just fits the viewport (same math as `fit`).
    #[must_use]
    pub fn min_zoom(&self, canvas_w: f64, canvas_h: f64) -> f64 {
        (canvas_w / self.nx).min(canvas_h / self.ny)
    }

    /// Zoom-in ceiling: at most `MIN_SPAN_PIXELS` image pixels visible along
    /// either axis. Beyond this the image is a blur of a handful of texels.
    #[must_use]
    #[allow(clippy::unused_self)] // symmetric with min_zoom, image-scoped API
    pub fn max_zoom(&self, canvas_w: f64, canvas_h: f64) -> f64 {
        (canvas_w / MIN_SPAN_PIXELS).min(canvas_h / MIN_SPAN_PIXELS)
    }

    /// Clamp `center` so the visible viewport never exposes the letterbox
    /// beyond the image bounds. When the image is smaller than the viewport
    /// along an axis, pin to the image midpoint on that axis.
    fn clamp_center(&mut self, canvas_w: f64, canvas_h: f64) {
        let half_x = canvas_w / (2.0 * self.zoom);
        let half_y = canvas_h / (2.0 * self.zoom);
        if half_x * 2.0 >= self.nx {
            self.center[0] = self.nx / 2.0;
        } else {
            self.center[0] = self.center[0].clamp(half_x, self.nx - half_x);
        }
        if half_y * 2.0 >= self.ny {
            self.center[1] = self.ny / 2.0;
        } else {
            self.center[1] = self.center[1].clamp(half_y, self.ny - half_y);
        }
    }

    /// Column-major 3×3 (as 9 floats) mapping image space → clip space, given
    /// the canvas size. Image y goes UP on screen (astronomy FITS convention:
    /// row 0 at bottom); combined with the un-flipped shader sample, texture
    /// row 0 and boundary y=0 both land at the bottom of the canvas.
    #[must_use]
    pub fn matrix(&self, canvas_w: f64, canvas_h: f64) -> [f32; 9] {
        // clip.x = (img.x - cx) * zoom * 2/canvas_w
        // clip.y = (img.y - cy) * zoom * 2/canvas_h   (y-up: larger img.y → higher)
        let sx = self.zoom * 2.0 / canvas_w;
        let sy = self.zoom * 2.0 / canvas_h;
        let (cx, cy) = (self.center[0], self.center[1]);
        [
            sx as f32,
            0.0,
            0.0,
            0.0,
            sy as f32,
            0.0,
            (-cx * sx) as f32,
            (-cy * sy) as f32,
            1.0,
        ]
    }

    /// Pan by a canvas-pixel delta (e.g. a mouse drag), in image-space units.
    /// Grab-and-drag: content follows the cursor. Under y-up display, a downward
    /// canvas drag (dy > 0) must INCREASE center y so the visible frame moves
    /// toward larger image y — putting content that was above the cursor under it.
    /// After the delta, `center` is clamped so the viewport never leaves the image.
    pub fn pan(&mut self, dx_canvas: f64, dy_canvas: f64, canvas_w: f64, canvas_h: f64) {
        self.center[0] -= dx_canvas / self.zoom;
        self.center[1] += dy_canvas / self.zoom;
        self.clamp_center(canvas_w, canvas_h);
    }

    /// Zoom by `factor` about a canvas point, keeping that point fixed under the
    /// cursor. `factor > 1` zooms in. Clamps to `[min_zoom, max_zoom]` and then
    /// re-clamps `center` in case the new zoom exposed a letterbox edge.
    pub fn zoom_about(&mut self, factor: f64, canvas_x: f64, canvas_y: f64, cw: f64, ch: f64) {
        let before = self.canvas_to_image(canvas_x, canvas_y, cw, ch);
        let (lo, hi) = (self.min_zoom(cw, ch), self.max_zoom(cw, ch));
        self.zoom = (self.zoom * factor).clamp(lo, hi);
        let after = self.canvas_to_image(canvas_x, canvas_y, cw, ch);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
        self.clamp_center(cw, ch);
    }

    /// Canvas pixel → image pixel coordinate. Exact inverse of `matrix`'s y-up
    /// forward map: solving `clip.y = (iy-cy)*sy` with `clip.y = 1 - 2·canvas_y/ch`
    /// gives `iy = cy - (canvas_y - ch/2)/zoom` (canvas y grows down, image y up).
    #[must_use]
    pub fn canvas_to_image(&self, canvas_x: f64, canvas_y: f64, cw: f64, ch: f64) -> [f64; 2] {
        [
            self.center[0] + (canvas_x - cw / 2.0) / self.zoom,
            self.center[1] - (canvas_y - ch / 2.0) / self.zoom,
        ]
    }

    /// Image pixel → canvas pixel. Forward map used for placing DOM overlays
    /// (centroid labels) that must track pan/zoom. Inverse of `canvas_to_image`.
    #[must_use]
    pub fn image_to_canvas(&self, ix: f64, iy: f64, cw: f64, ch: f64) -> [f64; 2] {
        [
            cw / 2.0 + (ix - self.center[0]) * self.zoom,
            ch / 2.0 - (iy - self.center[1]) * self.zoom,
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

    #[test]
    fn canvas_to_image_inverts_the_render_matrix() {
        // The click inverse must land where the forward matrix drew a point —
        // otherwise clicks map to a mirrored pixel and clump hit-testing fails
        // (boundaries render fine but clicking them selects nothing).
        let mut c = Camera::new(172, 221);
        let (cw, ch) = (1670.0, 922.0);
        c.fit(cw, ch);
        let m = c.matrix(cw, ch); // column-major 3x3 image -> clip
        for (ix, iy) in [(71.8, 19.9), (10.0, 200.0), (150.0, 5.0), (86.0, 110.5)] {
            let clip_x = f64::from(m[0]) * ix + f64::from(m[3]) * iy + f64::from(m[6]);
            let clip_y = f64::from(m[1]) * ix + f64::from(m[4]) * iy + f64::from(m[7]);
            // clip -> canvas (clip +1 = left/top; canvas y grows down).
            let canvas_x = f64::midpoint(clip_x, 1.0) * cw;
            let canvas_y = f64::midpoint(1.0, -clip_y) * ch;
            let back = c.canvas_to_image(canvas_x, canvas_y, cw, ch);
            // Tolerance is loose because matrix() returns f32 (WebGL uniform).
            assert!((back[0] - ix).abs() < 1e-3, "x round-trip {back:?} vs {ix}");
            assert!((back[1] - iy).abs() < 1e-3, "y round-trip {back:?} vs {iy}");
        }
    }

    #[test]
    fn pan_is_grab_and_drag_on_both_axes() {
        // Under the y-up render, content follows the cursor: a positive canvas
        // dx decreases center x (drag right → view shifts left in image space),
        // and a positive canvas dy INCREASES center y (drag down → view shifts
        // toward larger image y, which sits higher on screen).
        // Use an image much larger than the viewport so the pan isn't clipped
        // by the letterbox clamp — half-viewport in image coords must be < nx/2.
        let mut c = Camera::new(1000, 1000);
        c.fit(200.0, 200.0);
        c.zoom = 4.0; // half_viewport_img = 200/(2*4) = 25 → wide room to pan
        c.center = [500.0, 500.0];
        let before = c.center;
        c.pan(10.0, 10.0, 200.0, 200.0);
        assert!(c.center[0] < before[0], "drag right → center x decreases");
        assert!(c.center[1] > before[1], "drag down → center y increases");
        assert!((c.center[1] - (before[1] + 10.0 / c.zoom)).abs() < 1e-9);
    }

    #[test]
    fn pan_clamps_to_image_bounds() {
        // With image larger than the viewport, a huge pan cannot expose the
        // letterbox: center is clamped so the visible half-viewport stays
        // inside [half_x, nx - half_x].
        let mut c = Camera::new(400, 400);
        c.fit(200.0, 200.0);
        // Push zoom up so image is bigger than viewport, then try to escape.
        c.zoom = 1.0;
        c.center = [200.0, 200.0];
        c.pan(1e6, 1e6, 200.0, 200.0);
        let half = 200.0 / (2.0 * c.zoom);
        assert!(c.center[0] >= half && c.center[0] <= c.nx - half);
        assert!(c.center[1] >= half && c.center[1] <= c.ny - half);
    }

    #[test]
    fn zoom_out_clamps_at_fit() {
        let mut c = Camera::new(100, 100);
        c.fit(200.0, 200.0); // zoom = 2.0 == min_zoom
        let min = c.min_zoom(200.0, 200.0);
        c.zoom_about(0.01, 100.0, 100.0, 200.0, 200.0); // try to zoom way out
        assert!(
            (c.zoom - min).abs() < 1e-9,
            "zoom stopped at fit, got {}",
            c.zoom
        );
    }

    #[test]
    fn zoom_in_clamps_at_max() {
        let mut c = Camera::new(1000, 1000);
        c.fit(200.0, 200.0);
        let max = c.max_zoom(200.0, 200.0); // 200 / 5 = 40
        c.zoom_about(1e9, 100.0, 100.0, 200.0, 200.0); // try to zoom way in
        assert!(
            (c.zoom - max).abs() < 1e-9,
            "zoom stopped at max, got {}",
            c.zoom
        );
    }

    #[test]
    fn refit_tracks_latest_canvas_size() {
        // After a resize, canvas_to_image must map the NEW canvas center to the
        // image center — the camera tracks the latest size, not the first. This
        // is what Viewer::resize now enforces; without it, click hit-testing
        // desyncs from the backing store (the Retina selection bug).
        let mut c = Camera::new(100, 80);
        c.fit(200.0, 200.0);
        c.fit(600.0, 400.0); // simulate a resize
        let img = c.canvas_to_image(300.0, 200.0, 600.0, 400.0);
        assert!((img[0] - 50.0).abs() < 1e-9, "x center after refit");
        assert!((img[1] - 40.0).abs() < 1e-9, "y center after refit");
    }

    #[test]
    fn image_to_canvas_roundtrips() {
        // image_to_canvas is the forward inverse of canvas_to_image; the two
        // must round-trip so DOM overlays (centroid labels) land on the same
        // pixels the WebGL centroid draws.
        let mut c = Camera::new(500, 400);
        c.fit(1000.0, 800.0);
        c.zoom = 2.5;
        c.center = [220.0, 175.0];
        for (ix, iy) in [(50.0, 60.0), (250.0, 200.0), (499.0, 0.0), (10.5, 391.7)] {
            let [cx, cy] = c.image_to_canvas(ix, iy, 1000.0, 800.0);
            let back = c.canvas_to_image(cx, cy, 1000.0, 800.0);
            assert!((back[0] - ix).abs() < 1e-9, "x drift {back:?}");
            assert!((back[1] - iy).abs() < 1e-9, "y drift {back:?}");
        }
    }
}
