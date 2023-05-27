use std::mem::transmute;
use std::ptr::null_mut;

use ash::prelude::VkResult;
use ash::vk;
use log::{debug, error, info, trace, warn};

use crate::dpb::Dpb;
use crate::profile::VideoProfile;
use crate::session_parameters::make_h264_video_session_parameters;
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

pub struct VideoSession<'a> {
    session: vk::VideoSessionKHR,
    profile: Box<VideoProfile<'a>>,
    memories: Vec<vk::DeviceMemory>,
    parameters: Option<vk::VideoSessionParametersKHR>,
    is_encode: bool,
    codec: Codec,
}

impl VideoSession<'_> {
    pub fn is_encode(&self) -> &bool {
        &self.is_encode
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn parameters(&self) -> Option<vk::VideoSessionParametersKHR> {
        self.parameters
    }

    pub fn session(&self) -> vk::VideoSessionKHR {
        self.session
    }

    pub fn profile(&self) -> &Box<VideoProfile<'_>> {
        &self.profile
    }
}

struct SwapChainData<'a> {
    dpb: VkResult<Dpb>,
    _video_max_extent: vk::Extent2D,
    _swapchain_format: vk::Format,
    //swapchain_color_space: vk::ColorSpacekHz,
    encode_session: VkResult<VideoSession<'a>>,
    decode_session: VkResult<VideoSession<'a>>,
    _images: VkResult<Vec<vk::Image>>,
    image_views: VkResult<Vec<vk::ImageView>>,
    semaphores: Vec<VkResult<vk::Semaphore>>,
    _frame_index: u64,
}

impl SwapChainData<'_> {
    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        if let Ok(views) = self.image_views.as_mut() {
            for view in views.drain(..) {
                unsafe {
                    device.destroy_image_view(view, allocator);
                }
            }
        }
        if let Ok(dpb) = self.dpb.as_mut() {
            dpb.destroy(device, allocator);
        }

        for semaphore in self.semaphores.drain(..) {
            if let Ok(semaphore) = semaphore {
                unsafe { device.destroy_semaphore(semaphore, allocator) };
            }
        }
    }

    pub fn encode_image(
        &mut self,
        device: &ash::Device,
        video_queue_fn: &vk::KhrVideoQueueFn,
        video_encode_queue_fn: &vk::KhrVideoEncodeQueueFn,
        quality_level: u32,
        swapchain_index: usize,
        compute_queue: vk::Queue,
        encode_queue: vk::Queue,
        present_info: &vk::PresentInfoKHR,
    ) {
        if let (Ok(views), Ok(dpb), Ok(encode_session)) =
            (&self.image_views, &mut self.dpb, &self.encode_session)
        {
            let wait_semaphore_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(unsafe {
                    *present_info
                        .p_wait_semaphores
                        .as_ref()
                        .unwrap_or(&vk::Semaphore::null())
                })
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let semaphore = self.semaphores[swapchain_index];
            if let Ok(semaphore) = semaphore {
                let signal_semaphore_infos =
                    [vk::SemaphoreSubmitInfo::default().semaphore(semaphore)];

                let present_view = views[swapchain_index];
                let err = dpb.encode_frame(
                    device,
                    video_queue_fn,
                    video_encode_queue_fn,
                    encode_session,
                    quality_level,
                    present_view,
                    compute_queue,
                    encode_queue,
                    &wait_semaphore_infos,
                    &signal_semaphore_infos,
                );
                if let Err(err) = err {
                    error!("Failed to encode frame: {err}");
                }
            } else {
                error!("Something is terribly wrong: a semaphore is missing!");
            }
        }
    }
}

pub unsafe fn record_vk_create_swapchain(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> vk::Result {
    let allocator = p_allocator.as_ref();
    let lock = get_state().swapchain_fn.read().unwrap();
    let swapchain_fn = lock.as_ref().unwrap();
    let result =
        (swapchain_fn.create_swapchain_khr)(device, p_create_info, p_allocator, p_swapchain);

    if result == vk::Result::SUCCESS {
        info!("Created swapchain");
        let slot = get_state().private_slot.read().unwrap();
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let lock = get_state().physical_device.read().unwrap();
        let physical_device = lock.as_ref().unwrap();
        let lock = get_state().instance.read().unwrap();
        let instance = lock.as_ref().unwrap();

        let physical_memory_props =
            instance.get_physical_device_memory_properties(*physical_device);
        let create_info = p_create_info.as_ref().unwrap();
        //let swapchain_color_space =
        let swapchain_data = Box::new({
            let images = get_swapchain_images(device, swapchain_fn, *p_swapchain);

            let mut view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(create_info.image_format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });

            let image_views: VkResult<Vec<vk::ImageView>> = {
                if let Ok(images) = &images {
                    images
                        .iter()
                        .map(|&image| {
                            view_info.image = image;
                            device.create_image_view(&view_info, allocator)
                        })
                        .collect()
                } else {
                    Err(vk::Result::ERROR_INITIALIZATION_FAILED)
                }
            };

            let video_format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
            let encode_session = create_video_session(
                *get_state().encode_queue_family_idx.read().unwrap(),
                create_info.image_extent,
                create_info.image_extent,
                video_format,
                true,
                p_allocator,
            );
            let decode_session = create_video_session(
                *get_state().decode_queue_family_idx.read().unwrap(),
                create_info.image_extent,
                create_info.image_extent,
                video_format,
                false,
                p_allocator,
            );
            let swapchain_format = create_info.image_format;
            let mut dpb = encode_session.as_ref().map_err(|e| *e).and_then(|s| {
                Dpb::new(
                    device,
                    video_format,
                    create_info.image_extent,
                    16,
                    create_info.min_image_count,
                    p_allocator.as_ref(),
                    *get_state().encode_queue_family_idx.read().unwrap(),
                    *get_state().decode_queue_family_idx.read().unwrap(),
                    *get_state().compute_queue_family_idx.read().unwrap(),
                    s,
                    &physical_memory_props,
                )
            });
            let present_family_idx = *get_state().graphics_queue_family_idx.read().unwrap();
            if let (Ok(dpb), Ok(images), Ok(image_views)) =
                (dpb.as_mut(), images.as_ref(), image_views.as_ref())
            {
                if let Err(err) = dpb.prerecord_input_image_conversions(
                    device,
                    images,
                    image_views,
                    swapchain_format,
                    present_family_idx,
                    present_family_idx,
                ) {
                    error!("Failed to prerecord image conversions: {err}");
                }
            }

            let info = vk::SemaphoreCreateInfo::default();
            let semaphores = (0..create_info.min_image_count) // TODO: image count might be higher
                .map(|_| {
                    device.create_semaphore(&info, allocator).map_err(|err| {
                        error!("Failed to create present semaphore: {err}");
                        err
                    })
                })
                .collect();

            SwapChainData {
                _video_max_extent: create_info.image_extent,
                _swapchain_format: create_info.image_format,
                semaphores,
                dpb,
                encode_session,
                decode_session,
                _images: images,
                image_views,
                _frame_index: 0,
            }
        });
        let leaked = Box::leak(swapchain_data);
        if device
            .set_private_data(*p_swapchain, *slot, leaked as *const _ as u64)
            .is_err()
        {
            error!("Could not set private data!");
            Box::from_raw(leaked).destroy(device, allocator);
        }
    } else {
        warn!("Failed to create swapchain");
    }

    result
}

fn get_swapchain_images(
    device: &ash::Device,
    swapchain_fn: &vk::KhrSwapchainFn,
    swapchain: vk::SwapchainKHR,
) -> VkResult<Vec<vk::Image>> {
    unsafe {
        let mut len = 0;

        let res = (swapchain_fn.get_swapchain_images_khr)(
            device.handle(),
            swapchain,
            &mut len,
            null_mut(),
        );

        if res != vk::Result::SUCCESS {
            return Err(res);
        }

        let mut images = Vec::with_capacity(len as usize);
        images.set_len(len as usize);

        (swapchain_fn.get_swapchain_images_khr)(
            device.handle(),
            swapchain,
            &mut len,
            images.as_mut_ptr(),
        )
        .result_with_success(images)
    }
}

pub unsafe extern "system" fn record_vk_destroy_swapchain(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_allocator: *const vk::AllocationCallbacks,
) {
    let slot = get_state().private_slot.read().unwrap();
    let allocator = p_allocator.as_ref();
    {
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let lock = get_state().video_queue_fn.read().unwrap();
        let video_queue_fn = lock.as_ref().unwrap();
        let mut swapchain_data = Box::from_raw(transmute::<u64, *mut SwapChainData>(
            device.get_private_data(swapchain, *slot),
        ));
        swapchain_data.destroy(device, allocator);

        if let Ok(VideoSession {
            session,
            memories,
            parameters,
            ..
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
                device.free_memory(memory, allocator);
            }
        }
        if let Ok(VideoSession {
            session,
            memories,
            parameters,
            ..
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
                device.free_memory(memory, allocator);
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
    trace!("record_vk_queue_present");
    let lock = get_state().device.read().unwrap();
    let device = lock.as_ref().unwrap();
    let slot = get_state().private_slot.read().unwrap();
    let present_info = p_present_info.as_ref().unwrap();
    let lock = get_state().video_queue_fn.read().unwrap();
    let video_queue_fn = lock.as_ref().unwrap();
    let lock = get_state().video_encode_queue_fn.read().unwrap();
    let video_encode_queue_fn = lock.as_ref().unwrap();
    let quality_level = get_state().settings.quality_level;

    let swapchain_data = transmute::<u64, &mut SwapChainData>(
        device.get_private_data(*present_info.p_swapchains, *slot),
    );

    let compute_queue = *get_state().compute_queue.read().unwrap();
    let encode_queue = *get_state().encode_queue.read().unwrap();
    if let (Some(compute_queue), Some(encode_queue)) = (compute_queue, encode_queue) {
        swapchain_data.encode_image(
            device,
            video_queue_fn,
            video_encode_queue_fn,
            quality_level,
            *present_info.p_image_indices as usize,
            compute_queue,
            encode_queue,
            &present_info,
        );
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
    coded_extent: vk::Extent2D,
    video_format: vk::Format,
    is_encode: bool,
    p_allocator: *const vk::AllocationCallbacks,
) -> VkResult<VideoSession> {
    trace!("create_video_session");
    let state = get_state();
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

    let profile = VideoProfile::new(video_format, state.settings.codec, is_encode)?;
    let info = vk::VideoSessionCreateInfoKHR::default()
        .queue_family_index(queue_family_idx)
        .max_coded_extent(max_coded_extent)
        .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .max_dpb_slots(16)
        .max_active_reference_pictures(8)
        .std_header_version(&header_version)
        .video_profile(profile.profile());

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

    if let Err(err) = res {
        error!(
            "Failed to create {} video session: {err}",
            if is_encode { "encode" } else { "decode" }
        );
    } else {
        info!(
            "Created {} video session",
            if is_encode { "encode" } else { "decode" }
        );
    }

    res.and_then(|session| {
        Ok(VideoSession {
            is_encode,
            codec: state.settings.codec,
            session,
            profile,
            memories: {
                let mut memories = Vec::new();
                unsafe {
                    let mut len = 0;
                    let mut res = (video_queue_fn.get_video_session_memory_requirements_khr)(
                        device.handle(),
                        session,
                        &mut len,
                        null_mut(),
                    );

                    let mut requirements = Vec::new();

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
                    (true, Codec::H264) => make_h264_video_session_parameters(
                        device,
                        video_queue_fn,
                        session,
                        video_format,
                        coded_extent,
                        unsafe { p_allocator.as_ref() },
                    )
                    .ok(),
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
