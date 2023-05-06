[[vk::binding(0)]] Texture2D<float4> rgba;
[[vk::binding(1)]] SamplerState s;
[[vk::binding(2)]] RWTexture2D<float> y;

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID) {
  // Rec. 709 https://en.wikipedia.org/wiki/YCbCr
  y[id.xy] =
      dot(float3(0.2126, 0.7152, 0.0722), rgba.SampleLevel(s, id.xy, 0).rgb);
}
