void vs_egui(
    in const float2 i_pos  : POSITION,
    in const float2 i_uv   : TEXCOORD,
    in const float4 i_color: COLOR,
    out      float4 o_pos  : SV_POSITION,
    out      float2 o_uv   : TEXCOORD,
    out      float4 o_color: COLOR) {
    o_pos   = float4(i_pos, 0.0, 1.0);
    o_uv    = i_uv;
    o_color = i_color;
}

Texture2D<float4> g_texture: register(t0);
SamplerState      g_sampler: register(s0);

float4 ps_egui(
    in const float4 i_pos  : SV_POSITION,
    in const float2 i_uv   : TEXCOORD,
    in const float4 i_color: COLOR): SV_TARGET {
    return i_color * g_texture.Sample(g_sampler, i_uv);
}

// Explicit LOD avoids hardware-dependent implicit-LOD changes when UI
// meshes switch between base-only and mipmapped samplers on the same shader.
float4 ps_egui_minification(
    in const float4 i_pos  : SV_POSITION,
    in const float2 i_uv   : TEXCOORD,
    in const float4 i_color: COLOR): SV_TARGET {
    uint width, height;
    g_texture.GetDimensions(width, height);
    float2 dx = ddx(i_uv) * float2(width, height);
    float2 dy = ddy(i_uv) * float2(width, height);
    float footprint = max(dot(dx, dx), dot(dy, dy));
    if (footprint <= 1.0) {
        // Preserve the selected nearest/linear magnification exactly.
        return i_color * g_texture.Sample(g_sampler, i_uv);
    }
    float lod = 0.5 * log2(footprint);
    // Reduce trilinear softness by half a level, without falling back to raw
    // pixels once the footprint reaches two texels. No extra texture taps.
    lod = max(min(lod, 1.0), lod - 0.5);
    return i_color * g_texture.SampleLevel(g_sampler, i_uv, lod);
}
