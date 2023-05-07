[[vk::binding(0)]] Texture2D<float4> rgba;
[[vk::binding(1)]] SamplerState s;
[[vk::binding(2)]] RWTexture2D<float> y;
[[vk::binding(3)]] RWTexture2D<float2> uv;

[numthreads(8, 8, 1)]

void main(uint3 id: SV_DispatchThreadID) {

  float3 rgb = rgba.SampleLevel(s, id.xy, 0).rgb;
  // Rec. 709 https://en.wikipedia.org/wiki/YCbCr
  y[id.xy] = dot(float3(0.2126, 0.7152, 0.0722), rgb);

  float3 mean = 0.25 * (QuadReadLaneAt(rgb, 0) + QuadReadLaneAt(rgb, 1) +
                        QuadReadLaneAt(rgb, 2) + QuadReadLaneAt(rgb, 3));

  if ((id.x & 1) == 0 && (id.y & 1) == 0) {
    uv[id.xy / 2] = mul(
        float2x3(float3(-0.1146, -0.3854, 0.5), float3(0.5, -0.4542, -0.0458)),
        mean);
  }
}
