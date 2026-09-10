Texture2D source_texture : register(t0);
SamplerState source_sampler : register(s0);
cbuffer RasterTransform : register(b0) {
    float4 origin;
    float4 x_axis;
    float4 y_axis;
    float4 canvas;
};

float4 main(float4 position : SV_POSITION, float2 uv : TEXCOORD0) : SV_TARGET {
    float2 pixel = position.xy - 0.5;
    if (any(pixel >= canvas.xy)) return float4(0, 0, 0, 1);
    uint width, height;
    source_texture.GetDimensions(width, height);
    float2 size = float2(width, height);
    float2 sample_pixel = origin.xy + pixel.x * x_axis.xy + pixel.y * y_axis.xy;
    if (canvas.z == 0) {
        return float4(source_texture.Sample(source_sampler, (sample_pixel + 0.5) / size).rgb, 1);
    }
    // Match the export rotation's one-pixel border extension, clamping the integer
    // base before interpolation. It differs from clamping the whole sample position.
    float2 base = floor(sample_pixel);
    if (any(base < -1) || any(base > size)) return float4(0, 0, 0, 1);
    float2 fraction = sample_pixel - base;
    int2 first = int2(clamp(base, 0, size - 1));
    int2 second = min(first + 1, int2(size) - 1);
    float3 upper = lerp(source_texture.Load(int3(first, 0)).rgb,
                        source_texture.Load(int3(second.x, first.y, 0)).rgb, fraction.x);
    float3 lower = lerp(source_texture.Load(int3(first.x, second.y, 0)).rgb,
                        source_texture.Load(int3(second, 0)).rgb, fraction.x);
    // Export truncates its 8-bit bilinear result; do not introduce a rounding bias.
    return float4(floor(lerp(upper, lower, fraction.y) * 255.0 + 0.0001) / 255.0, 1);
}
