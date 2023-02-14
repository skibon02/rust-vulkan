#version 450 core

layout(location = 0) out vec4 outColor;
layout(location = 0) in vec2 fragTexCoord;

layout(binding = 0) uniform sampler2D tex;


void main() {
    outColor = texture(tex, fragTexCoord);
}