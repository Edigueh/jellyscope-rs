#version 300 es
// Overlay geometry (clump boundaries, centroids) in image pixel space,
// transformed by the same camera matrix as the image.
precision highp float;

layout(location = 0) in vec2 a_pos;

uniform mat3 u_view;

void main() {
    vec3 clip = u_view * vec3(a_pos, 1.0);
    gl_Position = vec4(clip.xy, 0.0, 1.0);
    gl_PointSize = 6.0; // centroid markers
}
