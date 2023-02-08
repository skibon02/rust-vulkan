#version 450 core

vec2 positions[3] = vec2[](
    vec2(0.0, -0.5),
    vec2(0.5, 0.5),
    vec2(-0.5, 0.5)
);
layout(location = 0) out vec3 fragColor;

layout(location = 0) in vec3 position;
layout(location = 1) in vec3 color;

void main() {
    gl_Position = vec4(position, 1.0);
    fragColor = color;
}