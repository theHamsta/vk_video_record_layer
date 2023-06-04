[[vk::binding(0), vk::image_format("rgba8")]] RWTexture2D<float4> rgba;
[[vk::binding(1), vk::image_format("r8")]] RWTexture2D<float> y;
[[vk::binding(2), vk::image_format("rg8")]] RWTexture2D<float2> uv;

[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  float3 rgb = rgba[id.xy].rgb;
  // Rec. 709 https://en.wikipedia.org/wiki/YCbCr
  y[id.xy] = dot(float3(0.2126, 0.7152, 0.0722), rgb);

  // requires subgroupBroadcastDynamicId=true as physDeviceFeature12
  float3 mean = 0.25 * (rgb + QuadReadLaneAt(rgb, 1) + QuadReadLaneAt(rgb, 2) +
                        QuadReadLaneAt(rgb, 3));

  if ((id.x & 1) == 0 && (id.y & 1) == 0) {
    // TODO: write 32bit by doing another shuffle?
    uv[id.xy / 2] = mul(
        float2x3(float3(-0.1146, -0.3854, 0.5), float3(0.5, -0.4542, -0.0458)),
        mean);
  }
}
