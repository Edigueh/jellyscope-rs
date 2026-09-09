//! Jellyscope WebGL viewer (WASM). JS drives it: construct with a canvas id,
//! push a baked RGBA texture and clump boundaries, then call `render` on
//! pan/zoom. All heavy science is pre-baked; this crate only samples textures,
//! moves the camera, and draws overlay lines.
//!
//! `unsafe` is confined to the `Float32Array::view` FFI sites in `gl.rs`.
//
// The casts below are WebGL-boundary conventions: texture/canvas dimensions
// (bounded, non-negative) to the `i32`/`f32` the GL API and shaders require.
// `JsValue` errors returned to JS are self-describing, so `# Errors` docs add
// nothing.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::similar_names
)]

mod camera;
mod gl;

use camera::Camera;
use gl::{ImageProgram, OverlayProgram};
use jelly_core::{composite, stretch};
use std::collections::HashSet;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlVertexArrayObject as Vao};

// Default boundary colour is white; user picks any RGB at runtime via
// `Viewer::set_boundary_color`. Selected boundaries stay hardcoded in the
// radialpaths --orange so selection reads clearly against any user colour.
const CLUMP_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const CLUMP_SELECTED: [f32; 4] = [0.836, 0.369, 0.0, 1.0];
// Centroid markers reuse the overlay shader; --teal for parity with the
// clump-list "inside" accent.
const CENTROID_COLOR: [f32; 4] = [0.165, 0.616, 0.561, 1.0];

/// One clump's vertex range within the overlay buffer, as `LINE_STRIP`.
struct Segment {
    id: i64,
    start: i32,
    count: i32,
}

#[wasm_bindgen]
pub struct Viewer {
    gl: Gl,
    image: ImageProgram,
    overlay: OverlayProgram,
    image_vao: Vao,
    overlay_vao: Vao,
    centroid_vao: Vao,
    camera: Camera,
    /// 0 = pass-through RGB (composite); 1..=6 = colormap applied to luminance.
    colormap_id: i32,
    canvas: HtmlCanvasElement,
    segments: Vec<Segment>,
    selected: HashSet<i64>,
    boundary_color: [f32; 4],
    show_centroids: bool,
    n_centroids: i32,
    // Raw flux planes (f64, row-major ny*nx), one per filter index. The viewer
    // stretches/composites these live; nothing is pre-baked.
    planes: Vec<Vec<f64>>,
    nx: usize,
    ny: usize,
    // GL buffers kept alive for the viewer's lifetime; never read directly.
    #[allow(dead_code)]
    quad_buf: web_sys::WebGlBuffer,
    #[allow(dead_code)]
    overlay_buf: Option<web_sys::WebGlBuffer>,
    #[allow(dead_code)]
    centroid_buf: Option<web_sys::WebGlBuffer>,
}

#[wasm_bindgen]
impl Viewer {
    /// Create a viewer bound to the WebGL2 context of the given canvas element.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str) -> Result<Viewer, JsValue> {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let canvas: HtmlCanvasElement = document
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str("canvas not found"))?
            .dyn_into()?;
        let gl: Gl = canvas
            .get_context("webgl2")?
            .ok_or_else(|| JsValue::from_str("no webgl2"))?
            .dyn_into()?;

        let image = ImageProgram::new(&gl)?;
        let overlay = OverlayProgram::new(&gl)?;

        // Image quad VAO: a unit quad as two triangles.
        let image_vao = gl
            .create_vertex_array()
            .ok_or_else(|| JsValue::from_str("vao"))?;
        gl.bind_vertex_array(Some(&image_vao));
        let quad: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
        let quad_buf = gl::upload_verts(&gl, &quad)?;

        let overlay_vao = gl
            .create_vertex_array()
            .ok_or_else(|| JsValue::from_str("vao"))?;
        let centroid_vao = gl
            .create_vertex_array()
            .ok_or_else(|| JsValue::from_str("vao"))?;

        Ok(Viewer {
            gl,
            image,
            overlay,
            image_vao,
            overlay_vao,
            centroid_vao,
            camera: Camera::new(1, 1),
            colormap_id: 1,
            canvas,
            segments: Vec::new(),
            selected: HashSet::new(),
            boundary_color: CLUMP_COLOR,
            show_centroids: false,
            n_centroids: 0,
            planes: Vec::new(),
            nx: 0,
            ny: 0,
            quad_buf,
            overlay_buf: None,
            centroid_buf: None,
        })
    }

    /// Start a new cube: set dimensions and clear the per-filter plane cache.
    #[wasm_bindgen(js_name = beginCube)]
    pub fn begin_cube(&mut self, nx: usize, ny: usize, n_filters: usize) {
        self.nx = nx;
        self.ny = ny;
        self.planes = vec![Vec::new(); n_filters];
    }

    /// Cache filter `index`'s flux plane. `bytes` is little-endian f32,
    /// `nx*ny*4`, row-major; widened to f64 for the exact bake math path.
    #[wasm_bindgen(js_name = loadPlane)]
    pub fn load_plane(
        &mut self,
        index: usize,
        nx: usize,
        ny: usize,
        bytes: &[u8],
    ) -> Result<(), JsValue> {
        if bytes.len() != nx * ny * 4 {
            return Err(JsValue::from_str("plane byte length != nx*ny*4"));
        }
        if index >= self.planes.len() {
            return Err(JsValue::from_str("plane index out of range"));
        }
        let plane = bytes
            .chunks_exact(4)
            .map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect();
        self.planes[index] = plane;
        Ok(())
    }

    /// Render a single filter: stretch its flux plane on the CPU (f64, identical
    /// to bake) → grayscale RGBA, colormapped in-shader. `stretch` = "log" |
    /// "asinh"; `colormap_id` = 1..=6.
    #[wasm_bindgen(js_name = renderSingle)]
    pub fn render_single(
        &mut self,
        index: usize,
        stretch: &str,
        colormap_id: i32,
    ) -> Result<(), JsValue> {
        let plane = self
            .planes
            .get(index)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| JsValue::from_str("filter plane not loaded"))?;
        let scalar = if stretch == "log" {
            stretch::log_stretch(plane)
        } else {
            stretch::lupton_asinh_stretch(plane)
        };
        let rgba = gray_rgba(&scalar);
        self.colormap_id = colormap_id;
        self.upload_and_show(&rgba)
    }

    /// Render an RGB composite from three cached planes on the CPU (f64,
    /// identical to bake). `method` = "percentile" | "lupton"; `softening` is the
    /// Lupton Q (ignored by percentile).
    #[wasm_bindgen(js_name = renderRgb)]
    pub fn render_rgb(
        &mut self,
        r: usize,
        g: usize,
        b: usize,
        method: &str,
        softening: f64,
    ) -> Result<(), JsValue> {
        let plane = |i: usize| {
            self.planes
                .get(i)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| JsValue::from_str("filter plane not loaded"))
        };
        let (rp, gp, bp) = (plane(r)?, plane(g)?, plane(b)?);
        let rgb = if method == "lupton" {
            composite::lupton(rp, gp, bp, softening)
        } else {
            composite::percentile_asinh(rp, gp, bp)
        };
        let rgba = rgb_to_rgba(&rgb);
        self.colormap_id = 0;
        self.upload_and_show(&rgba)
    }

    /// Upload interleaved RGBA bytes as the displayed texture and redraw. Refits
    /// the camera on the first load / dimension change.
    fn upload_and_show(&mut self, rgba: &[u8]) -> Result<(), JsValue> {
        let changed = self.camera.nx as usize != self.nx || self.camera.ny as usize != self.ny;
        let tex = gl::make_texture(&self.gl, self.nx, self.ny, rgba)?;
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
        if changed {
            self.camera = Camera::new(self.nx, self.ny);
            self.camera.fit(
                f64::from(self.canvas.width()),
                f64::from(self.canvas.height()),
            );
        }
        self.render();
        Ok(())
    }

    /// Set clump boundaries. `verts` is flat image-space `x,y` pairs; `counts`
    /// gives each clump's vertex count in order; `ids` the matching clump ids.
    /// Consecutive vertices of one clump form a closed `LINE_STRIP`.
    pub fn set_boundaries(
        &mut self,
        verts: &[f32],
        counts: &[i32],
        ids: &[i64],
    ) -> Result<(), JsValue> {
        self.gl.bind_vertex_array(Some(&self.overlay_vao));
        self.overlay_buf = Some(gl::upload_verts(&self.gl, verts)?);

        let mut segments = Vec::with_capacity(counts.len());
        let mut start = 0_i32;
        for (count, &id) in counts.iter().zip(ids) {
            segments.push(Segment {
                id,
                start,
                count: *count,
            });
            start += count;
        }
        self.segments = segments;
        self.render();
        Ok(())
    }

    /// Replace the selected clump set. Pass an empty slice to clear.
    #[wasm_bindgen(js_name = setSelected)]
    pub fn set_selected(&mut self, ids: &[i64]) {
        self.selected.clear();
        self.selected.extend(ids.iter().copied());
        self.render();
    }

    /// Override the non-selected clump boundary colour. Components in `[0,1]`.
    /// Selected clumps stay orange so selection is always visible.
    #[wasm_bindgen(js_name = setBoundaryColor)]
    pub fn set_boundary_color(&mut self, r: f32, g: f32, b: f32) {
        self.boundary_color = [r, g, b, 1.0];
        self.render();
    }

    /// Upload centroid positions (flat `x,y` pairs in image space) to a
    /// dedicated VAO. Rendered as `GL_POINTS` when `show_centroids` is on.
    #[wasm_bindgen(js_name = setCentroids)]
    pub fn set_centroids(&mut self, xs: &[f32]) -> Result<(), JsValue> {
        self.gl.bind_vertex_array(Some(&self.centroid_vao));
        self.centroid_buf = Some(gl::upload_verts(&self.gl, xs)?);
        self.n_centroids = (xs.len() / 2) as i32;
        self.render();
        Ok(())
    }

    /// Toggle centroid-marker visibility.
    #[wasm_bindgen(js_name = setShowCentroids)]
    pub fn set_show_centroids(&mut self, v: bool) {
        self.show_centroids = v;
        self.render();
    }

    /// Match the drawing-buffer size to the canvas CSS size (device pixels).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.gl.viewport(0, 0, width as i32, height as i32);
        // Re-fit so the camera's zoom/center track the new device-pixel backing
        // store that canvas_to_image / matrix / viewport all read; otherwise a
        // resize (e.g. Retina layout settling) desyncs them and click→pixel
        // hit-testing lands off-image.
        // ponytail: full re-fit resets pan/zoom on resize; rescale center/zoom
        // by the size ratio instead if preserving navigation across resizes matters.
        if self.nx > 0 && self.ny > 0 {
            self.camera.fit(f64::from(width), f64::from(height));
        }
        self.render();
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (w, h) = (
            f64::from(self.canvas.width()),
            f64::from(self.canvas.height()),
        );
        self.camera.pan(dx, dy, w, h);
        self.render();
    }

    pub fn zoom(&mut self, factor: f64, canvas_x: f64, canvas_y: f64) {
        let (w, h) = (
            f64::from(self.canvas.width()),
            f64::from(self.canvas.height()),
        );
        self.camera.zoom_about(factor, canvas_x, canvas_y, w, h);
        self.render();
    }

    /// Image pixel under a canvas point — JS uses this to index the pixmap for
    /// click→clump hit-testing.
    #[wasm_bindgen(js_name = canvasToImage)]
    #[must_use]
    pub fn canvas_to_image(&self, canvas_x: f64, canvas_y: f64) -> Vec<f64> {
        let (w, h) = (
            f64::from(self.canvas.width()),
            f64::from(self.canvas.height()),
        );
        self.camera
            .canvas_to_image(canvas_x, canvas_y, w, h)
            .to_vec()
    }

    pub fn render(&self) {
        let (w, h) = (
            f64::from(self.canvas.width()),
            f64::from(self.canvas.height()),
        );
        let m = self.camera.matrix(w, h);
        let g = &self.gl;
        g.clear_color(0.05, 0.05, 0.07, 1.0);
        g.clear(Gl::COLOR_BUFFER_BIT);

        // Image.
        g.use_program(Some(&self.image.program));
        g.bind_vertex_array(Some(&self.image_vao));
        g.uniform_matrix3fv_with_f32_array(Some(&self.image.u_view), false, &m);
        g.uniform2f(
            Some(&self.image.u_size),
            self.camera.nx as f32,
            self.camera.ny as f32,
        );
        g.uniform1i(Some(&self.image.u_tex), 0);
        g.uniform1i(Some(&self.image.u_colormap), self.colormap_id);
        g.draw_arrays(Gl::TRIANGLES, 0, 6);

        // Clump boundaries (selected drawn last, on top, in orange).
        if !self.segments.is_empty() {
            g.use_program(Some(&self.overlay.program));
            g.bind_vertex_array(Some(&self.overlay_vao));
            g.uniform_matrix3fv_with_f32_array(Some(&self.overlay.u_view), false, &m);
            for seg in &self.segments {
                if self.selected.contains(&seg.id) {
                    continue;
                }
                g.uniform4fv_with_f32_array(Some(&self.overlay.u_color), &self.boundary_color);
                g.draw_arrays(Gl::LINE_STRIP, seg.start, seg.count);
            }
            for seg in self
                .segments
                .iter()
                .filter(|s| self.selected.contains(&s.id))
            {
                g.uniform4fv_with_f32_array(Some(&self.overlay.u_color), &CLUMP_SELECTED);
                g.draw_arrays(Gl::LINE_STRIP, seg.start, seg.count);
            }
        }

        // Centroid markers (GL_POINTS, size set in the overlay vertex shader).
        if self.show_centroids && self.n_centroids > 0 {
            g.use_program(Some(&self.overlay.program));
            g.bind_vertex_array(Some(&self.centroid_vao));
            g.uniform_matrix3fv_with_f32_array(Some(&self.overlay.u_view), false, &m);
            g.uniform4fv_with_f32_array(Some(&self.overlay.u_color), &CENTROID_COLOR);
            g.draw_arrays(Gl::POINTS, 0, self.n_centroids);
        }
    }
}

/// Stretched scalar `[0,1]` (`NaN` = invalid) → grayscale RGBA. The shader
/// colormaps the luminance; `NaN` pixels get alpha 0 so they read transparent.
fn gray_rgba(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for &v in values {
        if v.is_nan() {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let g = (v.clamp(0.0, 1.0) * 255.0) as u8;
            out.extend_from_slice(&[g, g, g, 255]);
        }
    }
    out
}

/// Interleaved `RGB` bytes → `RGBA` (opaque; composites already blacken `NaN`).
fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for chunk in rgb.chunks_exact(3) {
        out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
    }
    out
}
