use std::marker::PhantomPinned;
use std::mem::transmute;

use crate::settings::Codec;
use ash::vk;

use ash::prelude::VkResult;

#[derive(Default)]
pub struct VideoProfile<'a> {
    profile: vk::VideoProfileInfoKHR<'a>,
    encode_usage: vk::VideoEncodeUsageInfoKHR<'a>,
    decode_usage: vk::VideoDecodeUsageInfoKHR<'a>,
    h264_encode_profile: vk::VideoEncodeH264ProfileInfoKHR<'a>,
    h264_decode_profile: vk::VideoDecodeH264ProfileInfoKHR<'a>,
    h265_encode_profile: vk::VideoEncodeH265ProfileInfoKHR<'a>,
    h265_decode_profile: vk::VideoDecodeH265ProfileInfoKHR<'a>,
    _marker: PhantomPinned, // self-referential pointers in this struct. Don't move in memory!
}

impl VideoProfile<'_> {
    pub fn new(video_format: vk::Format, codec: Codec, is_encode: bool) -> VkResult<Box<Self>> {
        assert_eq!(video_format, vk::Format::G8_B8R8_2PLANE_420_UNORM);
        let mut rtn: Box<Self> = Default::default();

        rtn.profile = vk::VideoProfileInfoKHR::default()
            .video_codec_operation(match (is_encode, codec) {
                (true, Codec::H264) => vk::VideoCodecOperationFlagsKHR::ENCODE_H264,
                (true, Codec::H265) => vk::VideoCodecOperationFlagsKHR::ENCODE_H265,
                (true, Codec::AV1) => todo!(),
                (false, Codec::H264) => vk::VideoCodecOperationFlagsKHR::DECODE_H264,
                (false, Codec::H265) => vk::VideoCodecOperationFlagsKHR::DECODE_H265,
                (false, Codec::AV1) => todo!(),
            })
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420);

        rtn.encode_usage = vk::VideoEncodeUsageInfoKHR::default()
            .video_usage_hints(vk::VideoEncodeUsageFlagsKHR::RECORDING)
            .video_content_hints(vk::VideoEncodeContentFlagsKHR::RENDERED)
            .tuning_mode(vk::VideoEncodeTuningModeKHR::HIGH_QUALITY);
        rtn.decode_usage = vk::VideoDecodeUsageInfoKHR::default()
            .video_usage_hints(vk::VideoDecodeUsageFlagsKHR::STREAMING);
        rtn.h264_encode_profile = vk::VideoEncodeH264ProfileInfoKHR::default()
            .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
        rtn.h265_encode_profile = vk::VideoEncodeH265ProfileInfoKHR::default();
        rtn.h264_decode_profile = vk::VideoDecodeH264ProfileInfoKHR::default()
            .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
        rtn.h265_decode_profile = vk::VideoDecodeH265ProfileInfoKHR::default();
        unsafe {
            if is_encode {
                match codec {
                    Codec::H264 => {
                        rtn.profile.p_next = transmute(&rtn.h264_encode_profile);
                    }
                    Codec::H265 => {
                        rtn.profile.p_next = transmute(&rtn.h265_encode_profile);
                    }
                    Codec::AV1 => todo!(),
                };
            } else {
                match codec {
                    Codec::H264 => {
                        rtn.profile.p_next = transmute(&rtn.h264_decode_profile);
                    }
                    Codec::H265 => {
                        rtn.profile.p_next = transmute(&rtn.h265_decode_profile);
                    }
                    Codec::AV1 => todo!(),
                };
            }
        }

        // validation layers don't like encode_usage, so put second
        unsafe {
            if is_encode {
                (*((rtn.profile.p_next) as *mut vk::BaseOutStructure)).p_next =
                    transmute(&rtn.encode_usage);
            } else {
                (*((rtn.profile.p_next) as *mut vk::BaseOutStructure)).p_next =
                    transmute(&rtn.decode_usage);
            }
        }

        Ok(rtn)
    }

    pub fn profile(&self) -> &vk::VideoProfileInfoKHR {
        &self.profile
    }
}
