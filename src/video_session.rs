use std::mem::transmute;

use ash::prelude::VkResult;
use ash::vk;
use log::{debug, info, trace, warn};

use crate::record_vk_get_device_proc_addr;
use crate::settings::Codec;

use crate::state::get_state;
use crate::vk_beta::{
    VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
};

struct SwapChainData {
    resolution: vk::Extent2D,
    video_max_extent: vk::Extent2D,
    swapchain_format: vk::Format,
    video_format: vk::Format,
    encode_session: VkResult<vk::VideoSessionKHR>,
    decode_session: VkResult<vk::VideoSessionKHR>,
}

impl SwapChainData {}

pub unsafe fn record_vk_create_swapchain(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> vk::Result {
    let result = (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .create_swapchain_khr)(device, p_create_info, p_allocator, p_swapchain);
    if result == vk::Result::SUCCESS {
        info!("Created swapchain");
        let slot = get_state().private_slot.read().unwrap();
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let create_info = p_create_info.as_ref().unwrap();
        /*let result = */
        device
            .set_private_data(
                *p_swapchain,
                *slot,
                Box::leak(Box::new(SwapChainData {
                    resolution: create_info.image_extent,
                    video_max_extent: create_info.image_extent,
                    swapchain_format: create_info.image_format,
                    video_format: vk::Format::G8_B8R8_2PLANE_420_UNORM,
                    encode_session: create_video_session(
                        *get_state().encode_queue_family_idx.read().unwrap(),
                        create_info.image_extent,
                        true,
                        p_allocator,
                    ),
                    decode_session: create_video_session(
                        *get_state().decode_queue_family_idx.read().unwrap(),
                        create_info.image_extent,
                        false,
                        p_allocator,
                    ),
                })) as *const _ as u64,
            )
            .unwrap(); // TODO
    } else {
        warn!("Failed to create swapchain");
    }

    result
}

pub unsafe extern "system" fn record_vk_destroy_swapchain(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_allocator: *const vk::AllocationCallbacks,
) {
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .destroy_swapchain_khr)(device, swapchain, p_allocator)
}

pub unsafe extern "system" fn record_vk_queue_present(
    queue: vk::Queue,
    p_present_info: *const vk::PresentInfoKHR,
) -> vk::Result {
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .queue_present_khr)(queue, p_present_info)
}

pub fn create_video_session(
    queue_family_idx: u32,
    max_coded_extent: vk::Extent2D,
    is_encode: bool,
    p_allocator: *const vk::AllocationCallbacks,
) -> VkResult<vk::VideoSessionKHR> {
    trace!("create_video_session");
    let state = get_state();
    let profile = vk::VideoProfileInfoKHR::default()
        .video_codec_operation(match (is_encode, state.settings.codec) {
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
    let header_version = match (is_encode, state.settings.codec) {
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
    let mut h265_encode_profile = vk::VideoEncodeH265ProfileInfoEXT::default();
    let mut h264_decode_profile = vk::VideoDecodeH264ProfileInfoKHR::default();
    let mut h265_decode_profile = vk::VideoDecodeH265ProfileInfoKHR::default();
    if is_encode {
        match state.settings.codec {
            Codec::H264 => profile.push_next(&mut h264_encode_profile),
            Codec::H265 => profile.push_next(&mut h265_encode_profile),
            Codec::AV1 => todo!(),
        };
    } else {
        match state.settings.codec {
            Codec::H264 => profile.push_next(&mut h264_decode_profile),
            Codec::H265 => profile.push_next(&mut h265_decode_profile),
            Codec::AV1 => todo!(),
        };
    }
    let info = vk::VideoSessionCreateInfoKHR::default()
        .queue_family_index(queue_family_idx)
        .max_coded_extent(max_coded_extent)
        .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .max_dpb_slots(16)
        .max_active_reference_pictures(0)
        .std_header_version(&header_version)
        .video_profile(&profile);

    let lock = state.device.read().unwrap();
    let device = lock.as_ref().unwrap();
    let mut lock = state.video_queue_fn.write().unwrap();
    let video_queue_fn = lock.as_mut().unwrap();

    let mut video_session = vk::VideoSessionKHR::null();
    let res = unsafe {
        (video_queue_fn.create_video_session_khr)(
            device.handle(),
            &info,
            p_allocator,
            &mut video_session,
        )
        .result_with_success(video_session)
    };

    if is_encode {
        info!("Create encode video session {res:?}");
    } else {
        info!("Create decode video session {res:?}");
    }

    res
}
