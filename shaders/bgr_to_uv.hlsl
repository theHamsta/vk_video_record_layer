[[vk::binding(0)]]
Texture2D<float4> rgba;
[[vk::binding(1)]]
SamplerState s;
[[vk::binding(2)]]
RWTexture2D<float2> uv;

[numthreads(8, 8, 1)]
void main( uint3 id : SV_DispatchThreadID ) {
    // Rec. 709 https://en.wikipedia.org/wiki/YCbCr
    uv[id.xy] = mul(float2x3(float3(-0.1146 , -0.3854 ,  0.5) , float3(0.5, -0.4542 , -0.0458)), rgba.SampleLevel(s, id.xy / 2, 0).rgb);
}

