#version 300 es
// Fullscreen-ish quad: two triangles covering the image rectangle in image
// pixel space, transformed to clip space by the camera matrix. `a_pos` is the
// unit quad [0,1]²; scaled to the image size by the caller via u_size.
precision highp float;

layout(location = 0) in vec2 a_pos;

uniform mat3 u_view;   // image space -> clip space
uniform vec2 u_size;   // image dimensions (nx, ny)

out vec2 v_uv;

void main() {
    vec2 img = a_pos * u_size;      // [0,nx]×[0,ny]
    vec3 clip = u_view * vec3(img, 1.0);
    gl_Position = vec4(clip.xy, 0.0, 1.0);
    v_uv = a_pos;                   // sample coords, y flipped below
}
