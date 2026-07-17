#version 300 es
// Samples the baked texture. For single-filter (grayscale) textures a Viridis
// colormap is applied to the luminance; for RGB composites the sampled colour
// is used directly. Transparent (alpha 0) texels — NaN pixels — stay clear.
precision highp float;

in vec2 v_uv;
out vec4 frag;

uniform sampler2D u_tex;
uniform int u_colormap;   // 1 = apply Viridis to luminance, 0 = pass-through RGB

// Polynomial Viridis approximation (Bhattacharjee/Mikhailov fit). No LUT.
vec3 viridis(float t) {
    const vec3 c0 = vec3(0.2777, 0.0054, 0.3341);
    const vec3 c1 = vec3(0.1050, 1.4046, 1.3845);
    const vec3 c2 = vec3(-0.3308, 0.2148, 0.0951);
    const vec3 c3 = vec3(-4.6342, -5.7991, -19.3324);
    const vec3 c4 = vec3(6.2282, 14.1799, 56.6906);
    const vec3 c5 = vec3(4.7763, -13.7451, -65.3532);
    const vec3 c6 = vec3(-5.4354, 4.6454, 26.3124);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

void main() {
    // Flip v so texture row 0 (FITS row 0) sits at image y = 0.
    vec4 texel = texture(u_tex, vec2(v_uv.x, 1.0 - v_uv.y));
    if (texel.a == 0.0) {
        discard; // NaN pixel
    }
    if (u_colormap == 1) {
        frag = vec4(viridis(texel.r), 1.0);
    } else {
        frag = vec4(texel.rgb, 1.0);
    }
}
