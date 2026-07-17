//! Jellyscope WebGL viewer (WASM). JS drives it: construct with a canvas id,
//! push a baked RGBA texture, then call `render` on pan/zoom. All heavy science
//! is pre-baked; this crate only samples textures and moves the camera.
//!
//! `unsafe` is confined to the one `Float32Array::view` FFI site in `gl.rs`.
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
use gl::ImageProgram;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

#[wasm_bindgen]
pub struct Viewer {
    gl: Gl,
    program: ImageProgram,
    camera: Camera,
    colormap: bool,
    canvas: HtmlCanvasElement,
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

        let program = ImageProgram::new(&gl)?;
        gl::make_quad(&gl)?;

        Ok(Viewer {
            gl,
            program,
            camera: Camera::new(1, 1),
            colormap: true,
            canvas,
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

    /// Image pixel under a canvas point — for click→clump hit-testing (step 9).
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
        let g = &self.gl;
        g.clear_color(0.05, 0.05, 0.07, 1.0);
        g.clear(Gl::COLOR_BUFFER_BIT);

        g.use_program(Some(&self.program.program));
        let m = self.camera.matrix(w, h);
        g.uniform_matrix3fv_with_f32_array(Some(&self.program.u_view), false, &m);
        g.uniform2f(
            Some(&self.program.u_size),
            self.camera.nx as f32,
            self.camera.ny as f32,
        );
        g.uniform1i(Some(&self.program.u_tex), 0);
        g.uniform1i(Some(&self.program.u_colormap), i32::from(self.colormap));
        g.draw_arrays(Gl::TRIANGLES, 0, 6);
    }
}
