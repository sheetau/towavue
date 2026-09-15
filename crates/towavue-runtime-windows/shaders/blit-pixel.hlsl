
Texture2D source_texture : register(t0);
SamplerState source_sampler : register(s0);
cbuffer Transform : register(b0) {
    float4 origin;
    float4 x_axis;
    float4 y_axis;
};

float4 main(float4 position : SV_POSITION, float2 uv : TEXCOORD0) : SV_TARGET {
    uint width, height;
    source_texture.GetDimensions(width, height);
    float2 half_texel = 0.5 / float2(width, height);
    float2 opposite = origin.xy + x_axis.xy + y_axis.xy;
    float2 sample_uv = origin.xy + uv.x * x_axis.xy + uv.y * y_axis.xy;
    return source_texture.Sample(source_sampler,
        clamp(sample_uv, min(origin.xy, opposite) + half_texel, max(origin.xy, opposite) - half_texel));
}
