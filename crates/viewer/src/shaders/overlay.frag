#version 300 es
// Flat-coloured overlay lines and points. Colour comes from a uniform so the
// same buffer can be drawn once per colour group (default / selected).
precision highp float;

uniform vec4 u_color;
out vec4 frag;

void main() {
    frag = u_color;
}
