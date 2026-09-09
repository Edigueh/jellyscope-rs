#version 300 es
// Samples the baked/live-composited texture. For single-filter (grayscale)
// textures a selectable colormap is applied to the luminance; for RGB
// composites (u_colormap == 0) the sampled colour is used directly. Transparent
// (alpha 0) texels — NaN pixels — stay clear.
precision highp float;

in vec2 v_uv;
out vec4 frag;

uniform sampler2D u_tex;
// 0 = pass-through RGB (composite); 1=Viridis 2=Inferno 3=Plasma 4=Cividis
// 5=Hot 6=Greys — matching the Python jellyscope colorscale options.
uniform int u_colormap;

// Polynomial colormap approximations (Matt Zucker / Mikhailov fits of the
// matplotlib maps). Each is a degree-6 polynomial in t ∈ [0,1]; no LUT.
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

vec3 inferno(float t) {
    const vec3 c0 = vec3(0.0002, 0.0016, -0.0194);
    const vec3 c1 = vec3(0.1065, 0.5639, 3.9327);
    const vec3 c2 = vec3(11.6024, -3.9728, -15.9423);
    const vec3 c3 = vec3(-41.7039, 17.4363, 44.3541);
    const vec3 c4 = vec3(77.1629, -33.4023, -81.8073);
    const vec3 c5 = vec3(-71.3194, 32.6261, 73.2095);
    const vec3 c6 = vec3(25.1311, -12.2426, -23.0703);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

vec3 plasma(float t) {
    const vec3 c0 = vec3(0.0580, 0.0229, 0.5280);
    const vec3 c1 = vec3(2.1765, 0.2384, 0.7539);
    const vec3 c2 = vec3(-2.6894, -7.4558, 3.1107);
    const vec3 c3 = vec3(6.1305, 42.3461, -28.5188);
    const vec3 c4 = vec3(-11.1074, -82.6663, 60.1398);
    const vec3 c5 = vec3(10.0233, 71.4136, -54.0721);
    const vec3 c6 = vec3(-3.6587, -22.9315, 18.1919);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

vec3 cividis(float t) {
    const vec3 c0 = vec3(-0.0106, 0.1391, 0.3049);
    const vec3 c1 = vec3(0.4144, 0.5910, 1.4300);
    const vec3 c2 = vec3(3.6304, -0.2811, -6.4034);
    const vec3 c3 = vec3(-11.6605, 0.6512, 15.8266);
    const vec3 c4 = vec3(19.2064, -0.8501, -18.7982);
    const vec3 c5 = vec3(-15.1120, 0.5334, 10.4045);
    const vec3 c6 = vec3(4.5320, -0.1435, -2.1876);
    return clamp(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), 0.0, 1.0);
}

// Matplotlib "hot": black -> red -> yellow -> white.
vec3 hot(float t) {
    return clamp(vec3(3.0 * t, 3.0 * t - 1.0, 3.0 * t - 2.0), 0.0, 1.0);
}

// Plotly "Greys" runs white (low) -> black (high): reversed grayscale.
vec3 greys(float t) {
    return vec3(1.0 - t);
}

void main() {
    // Sample the texture directly: bake writes each plane row-major from FITS
    // row 0 first, so texture v=0 IS FITS row 0, which the render matrix places
    // at image y=0 = top of screen. Boundary polygons (drawn as geometry through
    // the same matrix) share this frame → texture and overlays now agree.
    vec4 texel = texture(u_tex, v_uv);
    if (texel.a == 0.0) {
        discard; // NaN pixel
    }
    float t = clamp(texel.r, 0.0, 1.0);
    vec3 c;
    if      (u_colormap == 0) c = texel.rgb;   // composite pass-through
    else if (u_colormap == 1) c = viridis(t);
    else if (u_colormap == 2) c = inferno(t);
    else if (u_colormap == 3) c = plasma(t);
    else if (u_colormap == 4) c = cividis(t);
    else if (u_colormap == 5) c = hot(t);
    else                      c = greys(t);
    frag = vec4(c, 1.0);
}
