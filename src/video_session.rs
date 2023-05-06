use std::mem::transmute;
use std::ptr::null_mut;

use ash::prelude::VkResult;
use ash::vk;
use log::{debug, info, trace, warn};

use crate::settings::Codec;

use crate::state::get_state;
use crate::vk_beta::{
    /*StdVideoH264PictureParameterSet, StdVideoH264SequenceParameterSet, VkStructureType,
    VkVideoEncodeH264SessionParametersAddInfoEXT, VkVideoEncodeH264SessionParametersCreateInfoEXT,
    VkVideoSessionParametersCreateInfoKHR,*/
    VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
};

struct VideoSession {
    session: vk::VideoSessionKHR,
    memories: Vec<vk::DeviceMemory>,
    parameters: Option<vk::VideoSessionParametersKHR>,
}

struct SwapChainData {
    resolution: vk::Extent2D,
    video_max_extent: vk::Extent2D,
    swapchain_format: vk::Format,
    //swapchain_color_space: vk::ColorSpaceKHR,
    video_format: vk::Format,
    encode_session: VkResult<VideoSession>,
    decode_session: VkResult<VideoSession>,
    images: VkResult<Vec<vk::Image>>,
    image_views: VkResult<Vec<vk::ImageView>>,
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
        //let swapchain_color_space =
        /*let result = */
        device
            .set_private_data(
                *p_swapchain,
                *slot,
                Box::leak(Box::new({
                    let images = {
                        let mut images = Vec::new();
                        let mut len = 0;

                        let res = (get_state()
                            .swapchain_fn
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .get_swapchain_images_khr)(
                            device.handle(),
                            *p_swapchain,
                            &mut len,
                            null_mut(),
                        );

                        if res != vk::Result::SUCCESS {
                            return res;
                        }

                        images.resize(len as usize, vk::Image::null());

                        (get_state()
                            .swapchain_fn
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .get_swapchain_images_khr)(
                            device.handle(),
                            *p_swapchain,
                            &mut len,
                            images.as_mut_ptr(),
                        )
                        .result_with_success(images)
                    };
                    let image_views = {
                        if let Ok(images) = &images {
                            images
                                .iter()
                                .map(|&i| {
                                    device.create_image_view(
                                        &vk::ImageViewCreateInfo::default()
                                            .image(i)
                                            .view_type(vk::ImageViewType::TYPE_2D)
                                            .format(create_info.image_format)
                                            .components(vk::ComponentMapping {
                                                r: vk::ComponentSwizzle::R,
                                                g: vk::ComponentSwizzle::G,
                                                b: vk::ComponentSwizzle::B,
                                                a: vk::ComponentSwizzle::A,
                                            })
                                            .subresource_range(vk::ImageSubresourceRange {
                                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                                base_mip_level: 0,
                                                level_count: 1,
                                                base_array_layer: 0,
                                                layer_count: 1,
                                            }),
                                        p_allocator.as_ref(),
                                    )
                                })
                                .collect()
                        } else {
                            Err(vk::Result::ERROR_INITIALIZATION_FAILED)
                        }
                    };
                    SwapChainData {
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
                        images,
                        image_views,
                    }
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
    let slot = get_state().private_slot.read().unwrap();
    {
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let lock = get_state().video_queue_fn.read().unwrap();
        let video_queue_fn = lock.as_ref().unwrap();
        let swapchain_data = Box::from_raw(transmute::<u64, *mut SwapChainData>(
            device.get_private_data(swapchain, *slot),
        ));
        if let Ok(views) = swapchain_data.image_views {
            for view in views {
                device.destroy_image_view(view, p_allocator.as_ref());
            }
        }
        if let Ok(VideoSession {
            session,
            memories,
            parameters,
        }) = swapchain_data.decode_session
        {
            (video_queue_fn.destroy_video_session_khr)(device.handle(), session, p_allocator);
            if let Some(parameters) = parameters {
                (video_queue_fn.destroy_video_session_parameters_khr)(
                    device.handle(),
                    parameters,
                    p_allocator,
                );
            }
            for memory in memories {
                device.free_memory(memory, p_allocator.as_ref());
            }
        }
        if let Ok(VideoSession {
            session,
            memories,
            parameters,
        }) = swapchain_data.encode_session
        {
            (video_queue_fn.destroy_video_session_khr)(device.handle(), session, p_allocator);
            if let Some(parameters) = parameters {
                (video_queue_fn.destroy_video_session_parameters_khr)(
                    device.handle(),
                    parameters,
                    p_allocator,
                );
            }
            for memory in memories {
                device.free_memory(memory, p_allocator.as_ref());
            }
        }
    }
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
    let lock = get_state().device.read().unwrap();
    let device = lock.as_ref().unwrap();
    let slot = get_state().private_slot.read().unwrap();
    let present_info = p_present_info.as_ref().unwrap();

    let swapchain_data = transmute::<u64, &mut SwapChainData>(
        device.get_private_data(*present_info.p_swapchains, *slot),
    );
    if let Ok(images) = &swapchain_data.images {
        let _present_image = images[*present_info.p_image_indices as usize];
    }
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .queue_present_khr)(queue, p_present_info)
}

fn create_video_session(
    queue_family_idx: u32,
    max_coded_extent: vk::Extent2D,
    is_encode: bool,
    p_allocator: *const vk::AllocationCallbacks,
) -> VkResult<VideoSession> {
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
            .spec_version(vk::make_api_version(0, 0, 9, 9)),
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

    res.and_then(|session| {
        Ok(VideoSession {
            session,
            memories: {
                let mut memories = Vec::new();
                unsafe {
                    let mut requirements = Vec::new();
                    let mut len = requirements.len() as u32;
                    let mut res = (video_queue_fn.get_video_session_memory_requirements_khr)(
                        device.handle(),
                        session,
                        &mut len,
                        null_mut(),
                    );
                    if res == vk::Result::SUCCESS {
                        requirements.resize(len as usize, Default::default());
                        res = (video_queue_fn.get_video_session_memory_requirements_khr)(
                            device.handle(),
                            session,
                            &mut len,
                            requirements.as_mut_ptr(),
                        );
                        requirements.resize(len as usize, Default::default());
                    }
                    if res != vk::Result::SUCCESS {
                        (video_queue_fn.destroy_video_session_khr)(
                            device.handle(),
                            session,
                            p_allocator,
                        );
                        return Err(res);
                    }

                    let mut bind_infos = Vec::new();
                    // TODO try to "or" all memoryTypeBits to have them in as few allocations as
                    // possible? Not possible to have the in one. Or just use VMA?
                    for req in requirements.iter() {
                        debug!(
                            "memory_bind_index {}: requirements {:?}",
                            req.memory_bind_index, req.memory_requirements
                        );
                        let info = vk::MemoryAllocateInfo::default()
                            .allocation_size(req.memory_requirements.size)
                            .memory_type_index(
                                req.memory_requirements.memory_type_bits.trailing_zeros(),
                            );
                        let memory = device.allocate_memory(&info, p_allocator.as_ref());
                        match memory {
                            Ok(memory) => {
                                memories.push(memory);
                                bind_infos.push(
                                    vk::BindVideoSessionMemoryInfoKHR::default()
                                        .memory_bind_index(req.memory_bind_index)
                                        .memory(memory)
                                        .memory_size(req.memory_requirements.size),
                                );
                            }
                            Err(err) => {
                                res = err;
                                break;
                            }
                        }
                    }

                    if res == vk::Result::SUCCESS {
                        res = (video_queue_fn.bind_video_session_memory_khr)(
                            device.handle(),
                            session,
                            bind_infos.len() as u32,
                            bind_infos.as_ptr(),
                        );
                    }

                    if res != vk::Result::SUCCESS {
                        (video_queue_fn.destroy_video_session_khr)(
                            device.handle(),
                            session,
                            p_allocator,
                        );
                        for mem in memories.drain(..) {
                            device.free_memory(mem, p_allocator.as_ref());
                        }
                        return Err(res);
                    }
                }
                memories
            },
            parameters: {
                match (is_encode, state.settings.codec) {
                    (true, Codec::H264) => {
                        //let sps = vec![StdVideoH264SequenceParameterSet{
                        //flags: crate::vk_beta::StdVideoH264SpsFlags::default(),
                        //profile_idc: todo!(),
                        //level_idc: todo!(),
                        //chroma_format_idc: todo!(),
                        //seq_parameter_set_id: todo!(),
                        //bit_depth_luma_minus8: todo!(),
                        //bit_depth_chroma_minus8: todo!(),
                        //log2_max_frame_num_minus4: todo!(),
                        //pic_order_cnt_type: todo!(),
                        //offset_for_non_ref_pic: todo!(),
                        //offset_for_top_to_bottom_field: todo!(),
                        //log2_max_pic_order_cnt_lsb_minus4: todo!(),
                        //num_ref_frames_in_pic_order_cnt_cycle: todo!(),
                        //max_num_ref_frames: todo!(),
                        //reserved1: 0,
                        //pic_width_in_mbs_minus1: todo!(),
                        //pic_height_in_map_units_minus1: todo!(),
                        //frame_crop_left_offset: 0,
                        //frame_crop_right_offset: 0,
                        //frame_crop_top_offset: 0,
                        //frame_crop_bottom_offset: 0,
                        //reserved2: 0,
                        //pOffsetForRefFrame: todo!(),
                        //pScalingLists: todo!(),
                        //pSequenceParameterSetVui: todo!()
                        //}];
                        //let pps = vec![StdVideoH264PictureParameterSet{
                        //flags: todo!(),
                        //seq_parameter_set_id: 0,
                        //pic_parameter_set_id: 0,
                        //num_ref_idx_l0_default_active_minus1: todo!(),
                        //num_ref_idx_l1_default_active_minus1: todo!(),
                        //weighted_bipred_idc: todo!(),
                        //pic_init_qp_minus26: todo!(),
                        //pic_init_qs_minus26: todo!(),
                        //chroma_qp_index_offset: todo!(),
                        //second_chroma_qp_index_offset: todo!(),
                        //pScalingLists: todo!()
                        //}];
                        //let add_info = VkVideoEncodeH264SessionParametersAddInfoEXT {
                        //sType: VkStructureType::VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_ADD_INFO_EXT,
                        //pNext: null(),
                        //stdSPSCount: sps.len() as u32,
                        //pStdSPSs: sps.as_ptr(),
                        //stdPPSCount: pps.len()as u32,
                        //pStdPPSs: pps.as_ptr(),
                        //};
                        //let codec_info = VkVideoEncodeH264SessionParametersCreateInfoEXT {
                        //sType: VkStructureType::VK_STRUCTURE_TYPE_VIDEO_DECODE_H264_SESSION_PARAMETERS_CREATE_INFO_KHR,
                        //pNext: unsafe {transmute(&add_info)},
                        //maxStdSPSCount: todo!(),
                        //maxStdPPSCount: todo!(),
                        //pParametersAddInfo: todo!()
                        //};
                        //let info = VkVideoSessionParametersCreateInfoKHR {
                        //sType: todo!(),
                        //pNext: todo!(),
                        //flags: todo!(),
                        //videoSessionParametersTemplate: todo!(),
                        //videoSession: todo!()
                        //};
                        None
                    }
                    (true, Codec::H265) => None,
                    (true, Codec::AV1) => None,
                    (false, Codec::H264) => None,
                    (false, Codec::H265) => None,
                    (false, Codec::AV1) => None,
                }
            },
        })
    })
}
