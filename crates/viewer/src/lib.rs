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
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlVertexArrayObject as Vao};

// Clump boundary colours, matching the Python `image_viewer.py` (#00ccff /
// #ff4444) as normalized RGBA.
const CLUMP_COLOR: [f32; 4] = [0.0, 0.8, 1.0, 1.0];
const CLUMP_SELECTED: [f32; 4] = [1.0, 0.267, 0.267, 1.0];

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
    camera: Camera,
    colormap: bool,
    canvas: HtmlCanvasElement,
    segments: Vec<Segment>,
    selected: Option<i64>,
    // GL buffers kept alive for the viewer's lifetime; never read directly.
    #[allow(dead_code)]
    quad_buf: web_sys::WebGlBuffer,
    #[allow(dead_code)]
    overlay_buf: Option<web_sys::WebGlBuffer>,
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

        Ok(Viewer {
            gl,
            image,
            overlay,
            image_vao,
            overlay_vao,
            camera: Camera::new(1, 1),
            colormap: true,
            canvas,
            segments: Vec::new(),
            selected: None,
            quad_buf,
            overlay_buf: None,
        })
    }

    /// Replace the displayed texture. `is_rgb` selects pass-through colour;
    /// otherwise the grayscale luminance is Viridis-mapped. Resets the camera to
    /// fit on the first load / dimension change.
    pub fn set_texture(
        &mut self,
        nx: usize,
        ny: usize,
        rgba: &[u8],
        is_rgb: bool,
    ) -> Result<(), JsValue> {
        let changed = self.camera.nx as usize != nx || self.camera.ny as usize != ny;
        let tex = gl::make_texture(&self.gl, nx, ny, rgba)?;
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
        self.colormap = !is_rgb;
        if changed {
            self.camera = Camera::new(nx, ny);
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

    /// Highlight the clump with this id (red); pass a negative id to clear.
    pub fn set_selected(&mut self, id: i64) {
        self.selected = (id >= 0).then_some(id);
        self.render();
    }

    /// Match the drawing-buffer size to the canvas CSS size (device pixels).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.gl.viewport(0, 0, width as i32, height as i32);
        self.render();
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.camera.pan(dx, dy);
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
        g.uniform1i(Some(&self.image.u_colormap), i32::from(self.colormap));
        g.draw_arrays(Gl::TRIANGLES, 0, 6);

        // Clump boundaries (selected drawn last, on top, in red).
        if !self.segments.is_empty() {
            g.use_program(Some(&self.overlay.program));
            g.bind_vertex_array(Some(&self.overlay_vao));
            g.uniform_matrix3fv_with_f32_array(Some(&self.overlay.u_view), false, &m);
            for seg in &self.segments {
                if Some(seg.id) == self.selected {
                    continue;
                }
                g.uniform4fv_with_f32_array(Some(&self.overlay.u_color), &CLUMP_COLOR);
                g.draw_arrays(Gl::LINE_STRIP, seg.start, seg.count);
            }
            if let Some(sel) = self.selected {
                for seg in self.segments.iter().filter(|s| s.id == sel) {
                    g.uniform4fv_with_f32_array(Some(&self.overlay.u_color), &CLUMP_SELECTED);
                    g.draw_arrays(Gl::LINE_STRIP, seg.start, seg.count);
                }
            }
        }
    }
}
