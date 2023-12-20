use chrono::offset::Utc;
use chrono::DateTime;
use std::fs::File;
use std::mem::transmute;
use std::ptr::null_mut;
use std::time::SystemTime;

use ash::prelude::VkResult;
use ash::vk;
use log::{debug, error, info, trace, warn};

use crate::dpb::Dpb;
use crate::profile::VideoProfile;
use crate::session_parameters::{
    make_h264_video_session_parameters, make_h265_video_session_parameters,
};
use crate::settings::Codec;

use crate::state::{get_state, Extensions};

use crate::vulkan_utils::name_object;

pub struct VideoSession<'a> {
    session: vk::VideoSessionKHR,
    profile: Box<VideoProfile<'a>>,
    memories: Vec<vk::DeviceMemory>,
    parameters: Option<vk::VideoSessionParametersKHR>,
    codec: Codec,
    needs_reset: bool,
}

impl VideoSession<'_> {
    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn parameters(&self) -> Option<vk::VideoSessionParametersKHR> {
        self.parameters
    }

    pub fn session(&self) -> vk::VideoSessionKHR {
        self.session
    }

    pub fn profile(&self) -> &VideoProfile<'_> {
        &self.profile
    }

    fn destroy(
        &mut self,
        device: &ash::Device,
        video_queue_fn: &vk::KhrVideoQueueFn,
        allocator: Option<&vk::AllocationCallbacks>,
    ) {
        unsafe {
            for memory in self.memories.drain(..) {
                device.free_memory(memory, allocator);
            }
            (video_queue_fn.destroy_video_session_khr)(
                device.handle(),
                self.session,
                transmute(allocator),
            );
            if let Some(parameter) = self.parameters.take() {
                (video_queue_fn.destroy_video_session_parameters_khr)(
                    device.handle(),
                    parameter,
                    transmute(allocator),
                );
            }
        }
    }

    pub fn needs_reset(&self) -> bool {
        self.needs_reset
    }

    pub fn set_needs_reset(&mut self, needs_reset: bool) {
        self.needs_reset = needs_reset;
    }
}

struct SwapChainData<'a> {
    dpb: VkResult<Dpb>,
    _video_max_extent: vk::Extent2D,
    _swapchain_format: vk::Format,
    //swapchain_color_space: vk::ColorSpace,
    encode_session: VkResult<VideoSession<'a>>,
    decode_session: VkResult<VideoSession<'a>>,
    _images: VkResult<Vec<vk::Image>>,
    image_views: VkResult<Vec<vk::ImageView>>,
    semaphores: Vec<VkResult<vk::Semaphore>>,
    frame_index: u64,
    output_file: Option<File>,
}

impl SwapChainData<'_> {
    pub fn destroy(
        &mut self,
        device: &ash::Device,
        video_queue_fn: &vk::KhrVideoQueueFn,
        allocator: Option<&vk::AllocationCallbacks>,
    ) {
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

        for semaphore in self.semaphores.drain(..).flatten() {
            unsafe { device.destroy_semaphore(semaphore, allocator) };
        }

        for session in [&mut self.encode_session, &mut self.decode_session].iter_mut() {
            if let Ok(session) = session {
                session.destroy(device, video_queue_fn, allocator);
            }
        }
    }

    pub fn encode_image(
        &mut self,
        device: &ash::Device,
        extensions: &Extensions,
        swapchain_index: usize,
        compute_queue: vk::Queue,
        encode_queue: vk::Queue,
        present_info: &vk::PresentInfoKHR,
    ) {
        if let (Ok(views), Ok(dpb), Ok(encode_session)) =
            (&self.image_views, &mut self.dpb, &mut self.encode_session)
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
                let signal_semaphore_compute = [vk::SemaphoreSubmitInfo::default()
                    .semaphore(semaphore)
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];

                let present_view = views[swapchain_index];
                let err = dpb.encode_frame(
                    device,
                    extensions,
                    encode_session,
                    present_view,
                    compute_queue,
                    encode_queue,
                    &wait_semaphore_infos,
                    &signal_semaphore_compute,
                    self.output_file.as_mut(),
                );
                if let Err(err) = err {
                    error!("Failed to encode frame {}: {err:?}", self.frame_index);
                } else {
                    self.frame_index += 1;
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
    let extensions = get_state().extensions.read().unwrap();
    let swapchain_fn = extensions.swapchain_fn();
    let create_info = p_create_info.as_ref().unwrap();
    let create_info =
        create_info.image_usage(create_info.image_usage | vk::ImageUsageFlags::STORAGE);
    let result =
        (swapchain_fn.create_swapchain_khr)(device, &create_info, p_allocator, p_swapchain);

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
                        .enumerate()
                        .map(|(i, &image)| {
                            #[cfg(debug_assertions)]
                            name_object(
                                device,
                                &extensions,
                                image,
                                &format!("Swapchain image {i}"),
                            );
                            view_info.image = image;
                            let view = device.create_image_view(&view_info, allocator);

                            let _ = view.map(|view| {
                                #[cfg(debug_assertions)]
                                name_object(
                                    device,
                                    &extensions,
                                    view,
                                    &format!("Swapchain image view {i}"),
                                )
                            });
                            view
                        })
                        .collect()
                } else {
                    Err(vk::Result::ERROR_INITIALIZATION_FAILED)
                }
            };

            let video_format = vk::Format::G8_B8R8_2PLANE_420_UNORM;

            let output_folder = &get_state().settings.output_folder;
            let codec = &get_state().settings.codec;
            let lock = get_state().application_name.read().unwrap();
            let application_name = lock.as_ref().map(|s| s.as_str()).unwrap_or("UnknownApp");
            let time = SystemTime::now();
            let datetime: DateTime<Utc> = time.into();
            let vk::Extent2D { width, height } = create_info.image_extent;
            let codec_file_ext = match codec {
                Codec::H264 => "h264",
                Codec::H265 => "h265",
                Codec::AV1 => "av1",
            };
            let output_file = output_folder.join(format!(
                "{application_name}_{width}x{height}_{}.{codec_file_ext}",
                datetime.format("%d.%m.%Y_%H_%M_%S")
            ));
            info!("Starting output file: {output_file:?}");
            let output_file_handle = File::create(&output_file);

            debug!("Create encode session");
            let encode_session = create_video_session(
                *get_state().encode_queue_family_idx.read().unwrap(),
                create_info.image_extent,
                create_info.image_extent,
                video_format,
                true,
                output_file_handle.as_ref().ok(),
                p_allocator,
            );

            debug!("Create decode session");
            let decode_session = create_video_session(
                *get_state().decode_queue_family_idx.read().unwrap(),
                create_info.image_extent,
                create_info.image_extent,
                video_format,
                false,
                None,
                p_allocator,
            );
            let swapchain_format = create_info.image_format;
            let num_dpb_images = 1;
            let num_inflight_images = 10;
            let mut dpb = encode_session.as_ref().map_err(|e| *e).and_then(|s| {
                Dpb::new(
                    device,
                    &extensions,
                    video_format,
                    create_info.image_extent,
                    num_dpb_images,
                    num_inflight_images,
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
                frame_index: 0,
                output_file: output_file_handle.ok(),
            }
        });
        let leaked = Box::leak(swapchain_data);
        if device
            .set_private_data(*p_swapchain, *slot, leaked as *const _ as u64)
            .is_err()
        {
            error!("Could not set private data!");
            Box::from_raw(leaked).destroy(device, extensions.video_queue_fn(), allocator);
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

        let mut images = Vec::new();
        images.resize(len as usize, vk::Image::null());

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
    let extensions = get_state().extensions.read().unwrap();
    {
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let mut swapchain_data =
            Box::from_raw(device.get_private_data(swapchain, *slot) as *mut SwapChainData);
        swapchain_data.destroy(device, extensions.video_queue_fn(), allocator);
    }
    (extensions.swapchain_fn().destroy_swapchain_khr)(device, swapchain, p_allocator)
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
    let extensions = get_state().extensions.read().unwrap();

    let swapchain_data = transmute::<u64, &mut SwapChainData>(
        device.get_private_data(*present_info.p_swapchains, *slot),
    );

    let compute_queue = *get_state().compute_queue.read().unwrap();
    let encode_queue = *get_state().encode_queue.read().unwrap();
    if let (Some(compute_queue), Some(encode_queue)) = (compute_queue, encode_queue) {
        swapchain_data.encode_image(
            device,
            &extensions,
            *present_info.p_image_indices as usize,
            compute_queue,
            encode_queue,
            present_info,
        );
    }
    let info = p_present_info.as_ref().unwrap();
    let semaphores = [swapchain_data.semaphores[info.p_image_indices.read() as usize].unwrap()];
    let info = info.wait_semaphores(&semaphores);
    (extensions.swapchain_fn().queue_present_khr)(queue, &info)
}

fn create_video_session<'video_session>(
    queue_family_idx: u32,
    max_coded_extent: vk::Extent2D,
    coded_extent: vk::Extent2D,
    video_format: vk::Format,
    is_encode: bool,
    output_file: Option<&File>,
    p_allocator: *const vk::AllocationCallbacks,
) -> VkResult<VideoSession<'video_session>> {
    let state = get_state();
    trace!(
        "create_video_session {:?} {coded_extent:?}",
        state.settings.codec
    );
    let header_version = match (is_encode, state.settings.codec) {
        (true, Codec::H264) => vk::ExtensionProperties::default()
            .extension_name(vk::KhrVideoEncodeH264Fn::NAME)
            .unwrap()
            .spec_version(vk::make_api_version(0, 1, 0, 0)),
        (true, Codec::H265) => vk::ExtensionProperties::default()
            .extension_name(vk::KhrVideoEncodeH265Fn::NAME)
            .unwrap()
            .spec_version(vk::make_api_version(0, 1, 0, 0)),
        (true, Codec::AV1) => todo!(),
        (false, Codec::H264) => vk::ExtensionProperties::default()
            .extension_name(vk::KhrVideoDecodeH264Fn::NAME)
            .unwrap()
            .spec_version(vk::make_api_version(0, 1, 0, 0)),
        (false, Codec::H265) => vk::ExtensionProperties::default()
            .extension_name(vk::KhrVideoDecodeH265Fn::NAME)
            .unwrap()
            .spec_version(vk::make_api_version(0, 1, 0, 0)),
        (false, Codec::AV1) => todo!(),
    };

    let profile = VideoProfile::new(video_format, state.settings.codec, is_encode)?;
    let info = vk::VideoSessionCreateInfoKHR::default()
        .queue_family_index(queue_family_idx)
        .max_coded_extent(max_coded_extent)
        .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .max_dpb_slots(8)
        .max_active_reference_pictures(0)
        .std_header_version(&header_version)
        .video_profile(profile.profile());

    let lock = state.device.read().unwrap();
    let device = lock.as_ref().unwrap();
    let extensions = state.extensions.read().unwrap();
    let video_queue_fn = extensions.video_queue_fn();
    let encode_queue_fn = extensions.video_encode_queue_fn();

    let mut video_session = vk::VideoSessionKHR::null();
    let res = unsafe {
        (video_queue_fn.create_video_session_khr)(
            device.handle(),
            &info,
            p_allocator,
            &mut video_session,
        )
        .result_with_success(video_session)
        .map_err(|e| {
            error!(
                "Failed to create video session: {is_encode:?} {:?}",
                state.settings.codec
            );
            e
        })
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
            needs_reset: true,
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
                        encode_queue_fn,
                        session,
                        video_format,
                        coded_extent,
                        output_file,
                        unsafe { p_allocator.as_ref() },
                    )
                    .ok(),
                    (true, Codec::H265) => make_h265_video_session_parameters(
                        device,
                        video_queue_fn,
                        encode_queue_fn,
                        session,
                        video_format,
                        coded_extent,
                        output_file,
                        unsafe { p_allocator.as_ref() },
                    )
                    .ok(),
                    (true, Codec::AV1) => todo!(),
                    (false, Codec::H264) => None,
                    (false, Codec::H265) => None,
                    (false, Codec::AV1) => None,
                }
            },
        })
    })
}
