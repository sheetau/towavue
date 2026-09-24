// Each pass integrates the previous mip's texel areas. In particular, odd
// dimensions must not turn a one-pixel pattern into a large false pattern.
float4 vs_mipmap(uint id : SV_VertexID) : SV_POSITION {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

Texture2D<float4> source : register(t0);

float4 ps_mipmap(float4 position : SV_POSITION) : SV_TARGET {
    uint width, height;
    source.GetDimensions(width, height);
    uint2 destination = max(uint2(width, height) / 2, 1);
    float2 ratio = float2(width, height) / float2(destination);
    float2 begin = floor(position.xy) * ratio;
    float2 end = (floor(position.xy) + 1.0) * ratio;
    int2 first = int2(floor(begin));
    float4 sum = 0.0;
    // A halved integer extent overlaps at most three texels per axis.
    [unroll] for (int y = 0; y < 3; ++y) {
        [unroll] for (int x = 0; x < 3; ++x) {
            int2 pixel = first + int2(x, y);
            float2 weight = max(0.0, min(end, float2(pixel + 1)) - max(begin, float2(pixel)));
            sum += source.Load(int3(pixel, 0)) * weight.x * weight.y;
        }
    }
    return sum / (ratio.x * ratio.y);
}
