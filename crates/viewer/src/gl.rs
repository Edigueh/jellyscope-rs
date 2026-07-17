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

impl ImageProgram {
    pub fn new(gl: &Gl) -> Result<Self, JsValue> {
        let vert = compile(gl, Gl::VERTEX_SHADER, include_str!("shaders/image.vert"))?;
        let frag = compile(gl, Gl::FRAGMENT_SHADER, include_str!("shaders/image.frag"))?;
        let program = link(gl, &vert, &frag)?;
        let loc = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
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

/// A unit-quad VAO ([0,1]² as two triangles) for the image and any overlays.
pub fn make_quad(gl: &Gl) -> Result<(), JsValue> {
    let verts: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
    let vao = gl
        .create_vertex_array()
        .ok_or_else(|| JsValue::from_str("create_vao"))?;
    gl.bind_vertex_array(Some(&vao));
    let buf = gl
        .create_buffer()
        .ok_or_else(|| JsValue::from_str("create_buffer"))?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buf));
    // SAFETY: the view borrows `verts` only for this synchronous buffer_data call;
    // no allocation happens between the view and the copy into GL.
    unsafe {
        let view = js_sys::Float32Array::view(&verts);
        gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::STATIC_DRAW);
    }
    gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
    gl.enable_vertex_attrib_array(0);
    Ok(())
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
