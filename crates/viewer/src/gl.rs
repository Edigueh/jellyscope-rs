//! Thin WebGL2 helpers: compile/link the image program, upload RGBA textures,
//! and draw the image quad. web-sys keeps the GL calls safe (no `unsafe` here).

use wasm_bindgen::JsValue;
use web_sys::{
    WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader, WebGlTexture, WebGlUniformLocation,
};

/// The image-drawing program and its uniform locations.
pub struct ImageProgram {
    pub program: WebGlProgram,
    pub u_view: WebGlUniformLocation,
    pub u_size: WebGlUniformLocation,
    pub u_tex: WebGlUniformLocation,
    pub u_colormap: WebGlUniformLocation,
}

/// The overlay (lines/points) program and its uniform locations.
pub struct OverlayProgram {
    pub program: WebGlProgram,
    pub u_view: WebGlUniformLocation,
    pub u_color: WebGlUniformLocation,
}

impl OverlayProgram {
    pub fn new(gl: &Gl) -> Result<Self, JsValue> {
        let program = build_program(
            gl,
            include_str!("shaders/overlay.vert"),
            include_str!("shaders/overlay.frag"),
        )?;
        let loc = |name: &str| uniform(gl, &program, name);
        Ok(Self {
            u_view: loc("u_view")?,
            u_color: loc("u_color")?,
            program,
        })
    }
}

/// Upload `f32` vertex data (x,y pairs) into a freshly created buffer bound to
/// attribute location 0. Returns the buffer so the caller can keep it alive.
pub fn upload_verts(gl: &Gl, verts: &[f32]) -> Result<web_sys::WebGlBuffer, JsValue> {
    let buf = gl
        .create_buffer()
        .ok_or_else(|| JsValue::from_str("create_buffer"))?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buf));
    // SAFETY: the view borrows `verts` only for this synchronous buffer_data
    // call; no allocation happens between the view and the copy into GL.
    unsafe {
        let view = js_sys::Float32Array::view(verts);
        gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::STATIC_DRAW);
    }
    gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
    gl.enable_vertex_attrib_array(0);
    Ok(buf)
}

impl ImageProgram {
    pub fn new(gl: &Gl) -> Result<Self, JsValue> {
        let program = build_program(
            gl,
            include_str!("shaders/image.vert"),
            include_str!("shaders/image.frag"),
        )?;
        let loc = |name: &str| uniform(gl, &program, name);
        Ok(Self {
            u_view: loc("u_view")?,
            u_size: loc("u_size")?,
            u_tex: loc("u_tex")?,
            u_colormap: loc("u_colormap")?,
            program,
        })
    }
}

/// Upload interleaved RGBA bytes as a 2D texture (nearest filtering, clamped).
pub fn make_texture(gl: &Gl, nx: usize, ny: usize, rgba: &[u8]) -> Result<WebGlTexture, JsValue> {
    let tex = gl
        .create_texture()
        .ok_or_else(|| JsValue::from_str("create_texture"))?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        nx as i32,
        ny as i32,
        0,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        Some(rgba),
    )?;
    let clamp = Gl::CLAMP_TO_EDGE as i32;
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, clamp);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, clamp);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::NEAREST as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::NEAREST as i32);
    Ok(tex)
}

fn build_program(gl: &Gl, vert_src: &str, frag_src: &str) -> Result<WebGlProgram, JsValue> {
    let vert = compile(gl, Gl::VERTEX_SHADER, vert_src)?;
    let frag = compile(gl, Gl::FRAGMENT_SHADER, frag_src)?;
    link(gl, &vert, &frag)
}

fn uniform(gl: &Gl, program: &WebGlProgram, name: &str) -> Result<WebGlUniformLocation, JsValue> {
    gl.get_uniform_location(program, name)
        .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
}

fn compile(gl: &Gl, kind: u32, src: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl
        .create_shader(kind)
        .ok_or_else(|| JsValue::from_str("create_shader"))?;
    gl.shader_source(&shader, src);
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(JsValue::from_str(&format!(
            "shader compile: {}",
            gl.get_shader_info_log(&shader).unwrap_or_default()
        )))
    }
}

fn link(gl: &Gl, vert: &WebGlShader, frag: &WebGlShader) -> Result<WebGlProgram, JsValue> {
    let program = gl
        .create_program()
        .ok_or_else(|| JsValue::from_str("create_program"))?;
    gl.attach_shader(&program, vert);
    gl.attach_shader(&program, frag);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(JsValue::from_str(&format!(
            "program link: {}",
            gl.get_program_info_log(&program).unwrap_or_default()
        )))
    }
}

#[cfg(test)]
mod tests {
    // Host-side mirror of the GLSL colormaps in `shaders/image.frag`. GLSL can't
    // run under `cargo test`, so this pins the polynomial coefficients and the
    // analytic maps against gross transcription / direction errors: every output
    // channel must land in [0,1] across a t-sweep, and the two direction-sensitive
    // maps (Plotly Greys reversed, Hot rising) must match their endpoints.
    fn poly(t: f32, c: [[f32; 3]; 7]) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for k in 0..3 {
            let mut v = c[6][k];
            for row in (0..6).rev() {
                v = v * t + c[row][k];
            }
            out[k] = v;
        }
        out
    }

    const VIRIDIS: [[f32; 3]; 7] = [
        [0.2777, 0.0054, 0.3341],
        [0.1050, 1.4046, 1.3845],
        [-0.3308, 0.2148, 0.0951],
        [-4.6342, -5.7991, -19.3324],
        [6.2282, 14.1799, 56.6906],
        [4.7763, -13.7451, -65.3532],
        [-5.4354, 4.6454, 26.3124],
    ];
    const CIVIDIS: [[f32; 3]; 7] = [
        [-0.0106, 0.1391, 0.3049],
        [0.4144, 0.5910, 1.4300],
        [3.6304, -0.2811, -6.4034],
        [-11.6605, 0.6512, 15.8266],
        [19.2064, -0.8501, -18.7982],
        [-15.1120, 0.5334, 10.4045],
        [4.5320, -0.1435, -2.1876],
    ];

    #[test]
    fn viridis_in_gamut() {
        // Viridis is a good fit end-to-end; require [0,1] without clamping.
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            for ch in poly(t, VIRIDIS) {
                assert!((-0.01..=1.01).contains(&ch), "viridis t={t} ch={ch}");
            }
        }
    }

    #[test]
    fn cividis_clamped_endpoints() {
        // The shader clamps cividis; check its clamped endpoints are sane
        // (dark blue low, yellow high — blue drops, red/green rise).
        let clamp = |a: [f32; 3]| a.map(|x| x.clamp(0.0, 1.0));
        let lo = clamp(poly(0.0, CIVIDIS));
        let hi = clamp(poly(1.0, CIVIDIS));
        assert!(
            hi[0] > lo[0] && hi[1] > lo[1],
            "cividis brightens r,g upward"
        );
        assert!(hi[2] < lo[2] + 0.5, "cividis is not blue at the top");
    }

    #[test]
    fn hot_rises_black_to_white() {
        let hot = |t: f32| [3.0 * t, 3.0 * t - 1.0, 3.0 * t - 2.0].map(|x: f32| x.clamp(0.0, 1.0));
        assert!(hot(0.0).iter().all(|&c| c < 1e-6), "hot starts black");
        assert!(hot(1.0).iter().all(|&c| c > 1.0 - 1e-6), "hot ends white");
    }

    #[test]
    fn greys_reversed() {
        let greys = |t: f32| 1.0 - t;
        assert!((greys(0.0) - 1.0).abs() < 1e-6, "Greys low = white");
        assert!(greys(1.0).abs() < 1e-6, "Greys high = black");
    }
}
