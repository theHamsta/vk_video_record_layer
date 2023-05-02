use std::sync::RwLock;

use ash::vk;
use once_cell::sync::Lazy;

use crate::settings::{Codec, Settings};

use crate::vk_beta::{
    VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
};

// TODO: either do object wrapping or hash map dispatch. Until then there can only be a single
// instance/device
// or better: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VK_EXT_private_data.html (in core vk1.3)
#[derive(Default)]
pub struct State {
    pub instance: RwLock<Option<ash::Instance>>,
    pub device: RwLock<Option<ash::Device>>,
    pub swapchain_fn: RwLock<Option<vk::KhrSwapchainFn>>,
    pub instance_get_fn: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>>,
    pub device_get_fn: RwLock<Option<vk::PFN_vkGetDeviceProcAddr>>,
    pub settings: Settings,
    pub compute_queue: RwLock<Option<vk::Queue>>,
    pub encode_queue: RwLock<Option<vk::Queue>>,
    pub decode_queue: RwLock<Option<vk::Queue>>,
    pub private_slot: RwLock<vk::PrivateDataSlot>,
}

pub fn get_state() -> &'static State {
    static STATE: Lazy<State> = Lazy::new(|| {
        let mut state = State::default();
        state.settings = Settings::new_from_env();
        state
    });
    &STATE
}

impl State {
    pub fn create_video_session(
        &self,
        queue_family_idx: u32,
        max_coded_extent: vk::Extent2D,
        is_encode: bool,
    ) {
        let profile = vk::VideoProfileInfoKHR::default()
            .video_codec_operation(match (is_encode, self.settings.codec) {
                (true, Codec::H264) => vk::VideoCodecOperationFlagsKHR::ENCODE_H264_EXT,
                (true, Codec::H265) => vk::VideoCodecOperationFlagsKHR::ENCODE_H265_EXT,
                (true, Codec::AV1) => todo!(),
                (false, Codec::H264) => vk::VideoCodecOperationFlagsKHR::DECODE_H264,
                (false, Codec::H265) => vk::VideoCodecOperationFlagsKHR::DECODE_H265,
                (false, Codec::AV1) => todo!(),
            })
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420);
        let header_version = match (is_encode, self.settings.codec) {
            (true, Codec::H264) => vk::ExtensionProperties::default()
                .extension_name(unsafe {
                    *(VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME.as_ptr() as *const _)
                })
                .spec_version(vk::make_api_version(0, 0, 9, 8)),
            (true, Codec::H265) => vk::ExtensionProperties::default()
                .extension_name(unsafe {
                    *(VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME.as_ptr() as *const _)
                })
                .spec_version(vk::make_api_version(0, 0, 9, 9)),
            (true, Codec::AV1) => todo!(),
            (false, Codec::H264) => vk::ExtensionProperties::default()
                .extension_name(unsafe {
                    *(VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME.as_ptr() as *const _)
                })
                .spec_version(vk::make_api_version(0, 1, 0, 0)),
            (false, Codec::H265) => vk::ExtensionProperties::default()
                .extension_name(unsafe {
                    *(VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME.as_ptr() as *const _)
                })
                .spec_version(vk::make_api_version(0, 1, 0, 0)),
            (false, Codec::AV1) => todo!(),
        };
        let info = vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(queue_family_idx)
            .max_coded_extent(max_coded_extent)
            .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .max_dpb_slots(16)
            .max_active_reference_pictures(0)
            .std_header_version(&header_version);

        let mut encode_usage = vk::VideoEncodeUsageInfoKHR::default()
            .video_usage_hints(vk::VideoEncodeUsageFlagsKHR::RECORDING)
            .video_content_hints(vk::VideoEncodeContentFlagsKHR::RENDERED)
            .tuning_mode(vk::VideoEncodeTuningModeKHR::HIGH_QUALITY);
        let mut decode_usage = vk::VideoEncodeUsageInfoKHR::default()
            .video_usage_hints(vk::VideoEncodeUsageFlagsKHR::STREAMING);
        if is_encode {
            profile.push_next(&mut encode_usage);
        } else {
            profile.push_next(&mut decode_usage);
        }
        let mut h264_encode_profile = vk::VideoEncodeH264ProfileInfoEXT::default();
        let mut h265_encode_profile = vk::VideoEncodeH264ProfileInfoEXT::default();
        let mut h264_decode_profile = vk::VideoDecodeH264ProfileInfoKHR::default();
        let mut h265_decode_profile = vk::VideoDecodeH264ProfileInfoKHR::default();
        if is_encode {
            profile.push_next(match self.settings.codec {
                Codec::H264 => &mut h264_encode_profile,
                Codec::H265 => &mut h265_encode_profile,
                Codec::AV1 => todo!(),
            });
        } else {
            profile.push_next(match self.settings.codec {
                Codec::H264 => &mut h264_decode_profile,
                Codec::H265 => &mut h265_decode_profile,
                Codec::AV1 => todo!(),
            });
        }
        info.video_profile(&profile);
    }
}
