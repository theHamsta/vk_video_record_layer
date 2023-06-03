
[[vk::binding(0)]] Texture2D<float4> rgba;
[[vk::binding(1)]] SamplerState s;
[[vk::binding(2)]] RWTexture2D<uint> y;
[[vk::binding(3)]] RWTexture2D<uint2> uv;

[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  float3 rgb = rgba.SampleLevel(s, id.xy, 0).rgb;
  // Rec. 709 https://en.wikipedia.org/wiki/YCbCr
  y[id.xy] = 255;



  if ((id.x & 1) == 0 && (id.y & 1) == 0) {
    // TODO: write 32bit by doing another shuffle?
     uv[id.xy / 2].r = 255;
     uv[id.xy / 2].g = 255;
   }
}
