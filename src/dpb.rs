use crate::{
    shader::ComputePipelineDescriptor,
    vulkan_utils::{find_memorytype_index, name_object},
};
use anyhow::anyhow;
use ash::{prelude::VkResult, vk};
use itertools::Itertools;
use log::{debug, error};
use std::{
    collections::HashMap,
    io::Write,
    mem::{transmute, MaybeUninit},
    ptr::null,
};

use crate::{
    buffer_queue::{BitstreamBufferRing, BufferPair},
    cmd_buffer_queue::{CommandBuffer, CommandBufferQueue},
    settings::Codec,
    shader::ShaderPipeline,
    state::Extensions,
    video_session::VideoSession,
};

pub struct Dpb {
    extent: vk::Extent2D,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    y_views: Vec<vk::ImageView>,
    uv_views: Vec<vk::ImageView>,
    memory: Vec<vk::DeviceMemory>, // TODO: only one memory?
    compute_cmd_pool: vk::CommandPool,
    encode_cmd_pool: VkResult<CommandBufferQueue>,
    decode_cmd_pool: VkResult<CommandBufferQueue>,
    descriptor_pool: vk::DescriptorPool,
    next_image: u32,
    compute_family_index: u32,
    encode_family_index: u32,
    _decode_family_index: u32,
    compute_cmd_buffers: HashMap<(vk::ImageView, u32), vk::CommandBuffer>,
    sets: Vec<vk::DescriptorSet>,
    compute_pipeline: anyhow::Result<ComputePipelineDescriptor>,
    compute_shader: anyhow::Result<ShaderPipeline>,
    compute_semaphore: vk::Semaphore,
    encode_semaphore: vk::Semaphore,
    bitstream_buffers: VkResult<BitstreamBufferRing>,
    frame_index: u64,
}

#[derive(Debug, Copy, Clone)]
enum PictureType {
    Idr,
    I,
    #[allow(dead_code)]
    P,
    #[allow(dead_code)]
    B,
}

impl PictureType {
    fn as_h264_slice_type(self) -> vk::native::StdVideoH264SliceType {
        match self {
            PictureType::Idr | PictureType::I => {
                vk::native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_I
            }
            PictureType::P => vk::native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_P,
            PictureType::B => vk::native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_B,
        }
    }

    fn as_h265_slice_type(&self) -> vk::native::StdVideoH265SliceType {
        match self {
            PictureType::Idr | PictureType::I => {
                vk::native::StdVideoH265SliceType_STD_VIDEO_H265_SLICE_TYPE_I
            }
            PictureType::P => vk::native::StdVideoH265SliceType_STD_VIDEO_H265_SLICE_TYPE_P,
            PictureType::B => vk::native::StdVideoH265SliceType_STD_VIDEO_H265_SLICE_TYPE_B,
        }
    }

    fn as_h264_picture_type(self) -> vk::native::StdVideoH264PictureType {
        match self {
            PictureType::Idr => vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_IDR,
            PictureType::I => vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_I,
            PictureType::P => vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_P,
            PictureType::B => vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_B,
        }
    }

    fn as_h265_picture_type(&self) -> vk::native::StdVideoH265PictureType {
        match self {
            PictureType::Idr => vk::native::StdVideoH265PictureType_STD_VIDEO_H265_PICTURE_TYPE_IDR,
            PictureType::I => vk::native::StdVideoH265PictureType_STD_VIDEO_H265_PICTURE_TYPE_I,
            PictureType::P => vk::native::StdVideoH265PictureType_STD_VIDEO_H265_PICTURE_TYPE_P,
            PictureType::B => vk::native::StdVideoH265PictureType_STD_VIDEO_H265_PICTURE_TYPE_B,
        }
    }
    /// Returns `true` if the picture type is [`Idr`].
    ///
    /// [`Idr`]: PictureType::Idr
    #[must_use]
    fn is_idr(&self) -> bool {
        matches!(self, Self::Idr)
    }

    /// Returns `true` if the picture type is [`I`].
    ///
    /// [`I`]: PictureType::I
    #[must_use]
    #[allow(dead_code)]
    fn is_i(&self) -> bool {
        matches!(self, Self::I)
    }

    /// Returns `true` if the picture type is [`P`].
    ///
    /// [`P`]: PictureType::P
    #[allow(dead_code)]
    #[must_use]
    fn is_p(&self) -> bool {
        matches!(self, Self::P)
    }

    /// Returns `true` if the picture type is [`B`].
    ///
    /// [`B`]: PictureType::B
    #[must_use]
    #[allow(dead_code)]
    fn is_b(&self) -> bool {
        matches!(self, Self::B)
    }
}

impl Dpb {
    pub fn new(
        // src_queue_family_index
        device: &ash::Device,
        extensions: &Extensions,
        video_format: vk::Format,
        extent: vk::Extent2D,
        num_images: u32,
        max_input_image_views: u32,
        allocator: Option<&vk::AllocationCallbacks>,
        encode_family_index: u32,
        decode_family_index: u32,
        compute_family_index: u32,
        video_session: &VideoSession,
        physical_memory_props: &vk::PhysicalDeviceMemoryProperties,
    ) -> VkResult<Self> {
        unsafe {
            let mut images = Vec::new();
            let mut memory = Vec::new();
            let mut views = Vec::new();
            let mut y_views = Vec::new();
            let mut uv_views = Vec::new();
            let vk::Extent2D {
                mut width,
                mut height,
            } = extent;
            width = (width + 15) / 16 * 16;
            height = (height + 15) / 16 * 16;
            let mut res = vk::Result::SUCCESS;
            let indices = [
                encode_family_index,
                decode_family_index,
                compute_family_index,
            ];

            let info = vk::ImageCreateInfo::default()
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .format(video_format)
                .mip_levels(1)
                .array_layers(1)
                .image_type(vk::ImageType::TYPE_2D)
                .samples(vk::SampleCountFlags::TYPE_1)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .queue_family_indices(&indices)
                .usage(
                    vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR
                    | vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR
                    //vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR
                    // |vk::ImageUsageFlags::SAMPLED // requires samplerconversion pNext
                    | vk::ImageUsageFlags::STORAGE,
                )
                .tiling(vk::ImageTiling::OPTIMAL)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let profiles = [*video_session.profile().profile()];
            let mut profile_list = vk::VideoProfileListInfoKHR::default().profiles(&profiles);
            let info = info.push_next(&mut profile_list);

            let mut view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(video_format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let mut y_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::R8_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_0,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let mut uv_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::R8G8_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_1,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });

            for i in 0..num_images {
                let image = device.create_image(&info, allocator);
                let Ok(image) = image.map_err(|e| {
                    error!("Failed to create image for DPB: {e}");
                    res = e
                }) else {
                    break;
                };
                images.push(image);

                #[cfg(debug_assertions)]
                name_object(device, extensions, image, &format!("DPB image {}", i));

                let req = device.get_image_memory_requirements(image);
                let mem_index = find_memorytype_index(
                    &req,
                    physical_memory_props,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
                .ok_or(vk::Result::ERROR_INITIALIZATION_FAILED)?;

                let info = vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(mem_index);
                let Ok(mem) = device
                    .allocate_memory(&info, allocator)
                    .map_err(|e| res = e)
                else {
                    break;
                }; // TODO: one big allocation
                memory.push(mem);

                if let Err(err) = device.bind_image_memory(image, mem, 0) {
                    res = err;
                    break;
                }
                #[cfg(debug_assertions)]
                name_object(device, extensions, mem, &format!("DPB image memory {}", i));

                view_info.image = image;
                let view = device.create_image_view(&view_info, allocator);
                let Ok(view) = view.map_err(|e| {
                    error!("Failed to create color image view for DPB: {e}");
                    res = e
                }) else {
                    break;
                };
                views.push(view);

                #[cfg(debug_assertions)]
                name_object(device, extensions, view, &format!("DPB view {i}"));

                y_view_info.image = image;
                let view = device.create_image_view(&y_view_info, allocator);
                let Ok(view) = view.map_err(|e| {
                    error!("Failed to create luma image view for DPB: {e}");
                    res = e
                }) else {
                    break;
                };
                y_views.push(view);

                #[cfg(debug_assertions)]
                name_object(device, extensions, view, &format!("Y view {i}"));

                uv_view_info.image = image;
                let view = device.create_image_view(&uv_view_info, allocator);
                let Ok(view) = view.map_err(|e| {
                    error!("Failed to create chroma image view for DPB: {e}");
                    res = e
                }) else {
                    break;
                };
                uv_views.push(view);

                #[cfg(debug_assertions)]
                name_object(device, extensions, view, &format!("UV view {i}"));
            }

            let info =
                vk::CommandPoolCreateInfo::default().queue_family_index(compute_family_index);
            let compute_cmd_pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| {
                    error!("Failed to allocate compute command pool: {res}");
                    res = err
                })
                .unwrap_or(vk::CommandPool::null());

            let encode_cmd_pool = CommandBufferQueue::new(
                device,
                extensions,
                encode_family_index,
                10,
                100,
                "Encode command buffer",
                allocator,
            );
            let decode_cmd_pool = CommandBufferQueue::new(
                device,
                extensions,
                decode_family_index,
                10,
                100,
                "Decode command buffer",
                allocator,
            );

            let num_pools = max_input_image_views * num_images;
            let pool_sizes = vec![
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_IMAGE)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_IMAGE)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_IMAGE)
                    .descriptor_count(1),
            ];
            let info = vk::DescriptorPoolCreateInfo::default()
                .max_sets(num_pools)
                .pool_sizes(&pool_sizes);
            let descriptor_pool = device
                .create_descriptor_pool(&info, allocator)
                .map_err(|err| res = err)
                .unwrap_or(vk::DescriptorPool::null());
            let compute_shader = ShaderPipeline::new(
                device,
                &[include_bytes!("../shaders/bgr_to_yuv_rec709.hlsl.spirv")],
            );

            let compute_pipeline = if let Ok(shader) = compute_shader.as_ref() {
                shader
                    .make_compute_pipeline(device, "main", allocator)
                    .map_err(|e| {
                        error!("Failed to create compute pipeline: {e}");
                        e
                    })
            } else {
                error!("Failed to create compute pipeline!");
                Err(anyhow!("Missing shader"))
            };

            let mut timeline_info =
                vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
            let mut info = vk::SemaphoreCreateInfo::default();
            info = info.push_next(&mut timeline_info);
            let compute_semaphore = device
                .create_semaphore(&info, allocator)
                .map_err(|err| {
                    error!("Failed to create compute semaphore: {err}");
                    res = err;
                })
                .unwrap_or(vk::Semaphore::null());
            let encode_semaphore = device
                .create_semaphore(&info, allocator)
                .map_err(|err| {
                    error!("Failed to create encode semaphore: {err}");
                    res = err;
                })
                .unwrap_or(vk::Semaphore::null());

            let indices = [encode_family_index];
            let buffer_info = vk::BufferCreateInfo::default()
                .size(50000)
                .usage(
                    vk::BufferUsageFlags::VIDEO_ENCODE_DST_KHR | vk::BufferUsageFlags::TRANSFER_SRC,
                )
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .queue_family_indices(&indices)
                .push_next(&mut profile_list);
            let bitstream_buffers = BitstreamBufferRing::new(
                device,
                &buffer_info,
                30,
                physical_memory_props,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                encode_semaphore,
                &mut profiles[0].clone(),
                allocator,
            );

            let mut rtn = Self {
                next_image: 0,
                frame_index: 0,
                extent,
                images,
                views,
                y_views,
                uv_views,
                memory,
                compute_family_index,
                encode_family_index,
                _decode_family_index: decode_family_index,
                compute_cmd_pool,
                encode_cmd_pool,
                decode_cmd_pool,
                descriptor_pool,
                compute_cmd_buffers: Default::default(),
                compute_pipeline,
                compute_shader,
                compute_semaphore,
                encode_semaphore,
                bitstream_buffers,
                sets: Default::default(),
            };

            if res == vk::Result::SUCCESS {
                debug!("DPB resource successfully created!");
                Ok(rtn)
            } else {
                error!("Failed to create DPB resources: {res}!");
                rtn.destroy(device, allocator);
                Err(res)
            }
        }
    }

    pub fn prerecord_input_image_conversions(
        &mut self,
        device: &ash::Device,
        input_images: &[vk::Image],
        input_image_views: &[vk::ImageView],
        input_format: vk::Format,
        src_queue_family_index: u32,
        dst_queue_family_index: u32,
    ) -> anyhow::Result<()> {
        if input_format != vk::Format::B8G8R8A8_UNORM && input_format != vk::Format::B8G8R8A8_SRGB {
            panic!("Conversion for input format {input_format:?} not implemented yet");
        }

        // TODO: transition encode images to shader write
        unsafe {
            let info = vk::CommandBufferAllocateInfo::default()
                .command_pool(self.compute_cmd_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count((input_images.len() * self.views.len()) as u32);
            let mut cmds = device.allocate_command_buffers(&info)?;
            let compute_pipeline = self.compute_pipeline.as_ref().unwrap();
            for (&image, &view) in input_images.iter().zip(input_image_views) {
                for i in 0..self.views.len() {
                    let cmd = cmds.pop().unwrap();
                    let barriers = vec![
                        vk::ImageMemoryBarrier2::default()
                            .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                            .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                            .src_access_mask(vk::AccessFlags2::MEMORY_READ)
                            .dst_access_mask(vk::AccessFlags2::SHADER_STORAGE_READ)
                            .old_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                            .new_layout(vk::ImageLayout::GENERAL)
                            .src_queue_family_index(src_queue_family_index)
                            .dst_queue_family_index(self.compute_family_index)
                            .subresource_range(
                                vk::ImageSubresourceRange::default()
                                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                                    .base_mip_level(0)
                                    .level_count(1)
                                    .base_array_layer(0)
                                    .layer_count(1),
                            )
                            .image(image),
                        vk::ImageMemoryBarrier2::default()
                            .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                            .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                            .src_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR)
                            .dst_access_mask(vk::AccessFlags2::SHADER_STORAGE_WRITE)
                            .old_layout(vk::ImageLayout::UNDEFINED)
                            .new_layout(vk::ImageLayout::GENERAL)
                            .src_queue_family_index(self.encode_family_index)
                            .dst_queue_family_index(self.compute_family_index)
                            .subresource_range(
                                vk::ImageSubresourceRange::default()
                                    .aspect_mask(
                                        vk::ImageAspectFlags::COLOR
                                        //vk::ImageAspectFlags::PLANE_0 // TODO: 
                                            //| vk::ImageAspectFlags::PLANE_1,
                                    )
                                    .base_mip_level(0)
                                    .level_count(1)
                                    .base_array_layer(0)
                                    .layer_count(1),
                            )
                            .image(self.images[i]),
                    ];
                    let dep_info_present_to_compute =
                        vk::DependencyInfo::default().image_memory_barriers(&barriers);
                    let barriers = vec![vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                        .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                        .src_access_mask(vk::AccessFlags2::SHADER_STORAGE_READ)
                        .dst_access_mask(vk::AccessFlags2::MEMORY_READ)
                        .old_layout(vk::ImageLayout::GENERAL)
                        .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                        .src_queue_family_index(self.compute_family_index)
                        .dst_queue_family_index(dst_queue_family_index)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1),
                        )
                        .image(image)];
                    let dep_info_compute_to_present =
                        vk::DependencyInfo::default().image_memory_barriers(&barriers);
                    let info = vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::default());
                    device
                        .begin_command_buffer(cmd, &info)
                        .map_err(|err| anyhow!("Failed to begin command buffer: {err}"))?;
                    device.cmd_pipeline_barrier2(cmd, &dep_info_present_to_compute);
                    let info = vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.descriptor_pool)
                        .set_layouts(compute_pipeline.descriptor_set_layouts());

                    let set = device.allocate_descriptor_sets(&info)?;
                    self.sets.push(set[0]);
                    device.update_descriptor_sets(
                        &[
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(0)
                                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                                .image_info(&[vk::DescriptorImageInfo::default()
                                    .image_view(view)
                                    .image_layout(vk::ImageLayout::GENERAL)]),
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(1)
                                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                                .image_info(&[vk::DescriptorImageInfo::default()
                                    .image_view(self.y_views[i])
                                    .image_layout(vk::ImageLayout::GENERAL)]),
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(2)
                                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                                .image_info(&[vk::DescriptorImageInfo::default()
                                    .image_view(self.uv_views[i])
                                    .image_layout(vk::ImageLayout::GENERAL)]),
                        ],
                        &[],
                    );

                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        compute_pipeline.layout(),
                        0,
                        &[set[0]],
                        &[0],
                    );
                    device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        compute_pipeline.pipeline(),
                    );
                    device.cmd_push_constants(
                        cmd,
                        compute_pipeline.layout(),
                        vk::ShaderStageFlags::COMPUTE,
                        0,
                        &transmute::<_, [u8; 8]>(self.extent),
                    );
                    let extent = self.coded_extent();
                    device.cmd_dispatch(cmd, (extent.width + 7) / 8, (extent.height + 7) / 8, 1);
                    device.cmd_pipeline_barrier2(cmd, &dep_info_compute_to_present);
                    device
                        .end_command_buffer(cmd)
                        .map_err(|err| anyhow!("Failed to end command buffer: {err}"))?;

                    self.compute_cmd_buffers.insert((view, i as u32), cmd);
                }
            }
        }
        Ok(())
    }

    fn record_encode_cmd_buffer(
        &mut self,
        device: &ash::Device,
        extensions: &Extensions,
        buffer: &BufferPair,
        video_session: &mut VideoSession,
    ) -> anyhow::Result<CommandBuffer> {
        let video_queue_fn = extensions.video_queue_fn();
        let video_encode_queue_fn = extensions.video_encode_queue_fn();
        let cmd = self
            .encode_cmd_pool
            .as_mut()
            .map_err(|e| *e)?
            .next(device)?;
        unsafe {
            let cmd = cmd.cmd;
            let info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            device.begin_command_buffer(cmd, &info)?;

            let image = self.images[self.next_image as usize];
            let image_view = self.views[self.next_image as usize];
            let barriers = vec![vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
                .src_access_mask(vk::AccessFlags2::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::VIDEO_ENCODE_SRC_KHR)
                .src_queue_family_index(self.compute_family_index)
                .dst_queue_family_index(self.encode_family_index)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                )
                .image(image)];
            let info = vk::DependencyInfo::default().image_memory_barriers(&barriers);
            device.cmd_pipeline_barrier2(cmd, &info);

            let image_type = if video_session.needs_reset() {
                PictureType::Idr
            } else {
                PictureType::I
            };

            let info = vk::VideoBeginCodingInfoKHR::default()
                .video_session(video_session.session())
                .video_session_parameters(
                    video_session
                        .parameters()
                        .ok_or_else(|| anyhow!("Can't encode: missing VideoSessionParameters"))?,
                );
            (video_queue_fn.cmd_begin_video_coding_khr)(cmd, &info);

            if video_session.needs_reset() {
                let info = vk::VideoCodingControlInfoKHR::default()
                    .flags(vk::VideoCodingControlFlagsKHR::RESET);
                (video_queue_fn.cmd_control_video_coding_khr)(cmd, &info);
                video_session.set_needs_reset(false);
                // TODO: rate control
                //let mut rate_control = vk::VideoEncodeRateControlInfoKHR::default();
                //let info = vk::VideoCodingControlInfoKHR::default().push_next(&mut rate_control);
                //(video_queue_fn.cmd_control_video_coding_khr)(cmd, &info);
            }
            device.cmd_begin_query(
                cmd,
                buffer.query_pool,
                buffer.slot,
                vk::QueryControlFlags::default(),
            );

            let pic = vk::VideoPictureResourceInfoKHR::default()
                .coded_extent(self.coded_extent())
                .image_view_binding(image_view);
            let flags = MaybeUninit::zeroed();
            let mut flags: vk::native::StdVideoEncodeH264PictureInfoFlags = flags.assume_init();
            flags.set_IdrPicFlag(image_type.is_idr() as u32);
            let h264_pic = vk::native::StdVideoEncodeH264PictureInfo {
                flags,
                seq_parameter_set_id: 0,
                pic_parameter_set_id: 0,
                reserved1: [0; 3],
                frame_num: 0,
                PicOrderCnt: 0,
                idr_pic_id: 0,
                temporal_id: 0,
                primary_pic_type: image_type.as_h264_picture_type(),
                pRefLists: std::ptr::null(),
            };
            let flags = MaybeUninit::zeroed();
            let flags = flags.assume_init();
            let h264_header = vk::native::StdVideoEncodeH264SliceHeader {
                flags,
                first_mb_in_slice: 0,
                slice_type: image_type.as_h264_slice_type(),
                cabac_init_idc: 0,
                disable_deblocking_filter_idc: 0,
                slice_alpha_c0_offset_div2: 0,
                slice_beta_offset_div2: 0,
                reserved1: 0,
                slice_qp_delta: 0,
                pWeightTable: null(),
            };
            let h264_nalus =
                &[vk::VideoEncodeH264NaluSliceInfoEXT::default().std_slice_header(&h264_header)];
            let mut h264_info = vk::VideoEncodeH264PictureInfoEXT::default()
                .nalu_slice_entries(h264_nalus)
                .std_picture_info(&h264_pic);

            let flags = MaybeUninit::zeroed();
            let flags: vk::native::StdVideoEncodeH265PictureInfoFlags = flags.assume_init();
            let h265_pic = vk::native::StdVideoEncodeH265PictureInfo {
                flags,
                reserved1: Default::default(),
                pRefLists: std::ptr::null(),
                pic_type: image_type.as_h265_picture_type(),
                sps_video_parameter_set_id: 0,
                pps_seq_parameter_set_id: 0,
                pps_pic_parameter_set_id: 0,
                short_term_ref_pic_set_idx: 0,
                PicOrderCntVal: 0,
                TemporalId: 0,
                pShortTermRefPicSet: null(),
                pLongTermRefPics: null(),
            };
            let flags = MaybeUninit::zeroed();
            let mut flags: vk::native::StdVideoEncodeH265SliceSegmentHeaderFlags =
                flags.assume_init();
            flags.set_first_slice_segment_in_pic_flag(1);
            flags.set_slice_sao_luma_flag(1);
            flags.set_slice_sao_chroma_flag(1);
            flags.set_slice_deblocking_filter_disabled_flag(1);

            let h265_header = vk::native::StdVideoEncodeH265SliceSegmentHeader {
                flags,
                slice_type: image_type.as_h265_slice_type(),
                slice_beta_offset_div2: 0,
                reserved1: 0,
                slice_qp_delta: 0,
                pWeightTable: null(),
                slice_segment_address: 0,
                collocated_ref_idx: 0,
                MaxNumMergeCand: 0,
                slice_cb_qp_offset: 0,
                slice_cr_qp_offset: 0,
                slice_tc_offset_div2: 0,
                slice_act_y_qp_offset: 0,
                slice_act_cb_qp_offset: 0,
                slice_act_cr_qp_offset: 0,
            };
            let h265_nalus = &[vk::VideoEncodeH265NaluSliceSegmentInfoEXT::default()
                .std_slice_segment_header(&h265_header)];
            let mut h265_info = vk::VideoEncodeH265PictureInfoEXT::default()
                .nalu_slice_segment_entries(h265_nalus)
                .std_picture_info(&h265_pic);

            let mut info = vk::VideoEncodeInfoKHR::default()
                .dst_buffer(buffer.device.buffer())
                .dst_buffer_range(buffer.device.size())
                .src_picture_resource(pic);
            match video_session.codec() {
                Codec::H264 => info = info.push_next(&mut h264_info),
                Codec::H265 => info = info.push_next(&mut h265_info),
                Codec::AV1 => todo!(),
            };
            device.cmd_end_query(cmd, buffer.query_pool, buffer.slot);
            (video_encode_queue_fn.cmd_encode_video_khr)(cmd, &info);

            let info = vk::VideoEndCodingInfoKHR::default();
            (video_queue_fn.cmd_end_video_coding_khr)(cmd, &info);

            let barriers = [vk::BufferMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
                .src_access_mask(vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR)
                .src_queue_family_index(self.encode_family_index)
                .dst_stage_mask(vk::PipelineStageFlags2::TRANSFER)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                .dst_queue_family_index(self.encode_family_index)
                .buffer(buffer.device.buffer())
                .size(buffer.device.size())];
            let info = vk::DependencyInfo::default().buffer_memory_barriers(&barriers);
            device.cmd_pipeline_barrier2(cmd, &info);
            let copies = [vk::BufferCopy2::default().size(buffer.device.size())];
            let info = vk::CopyBufferInfo2::default()
                .src_buffer(buffer.device.buffer())
                .dst_buffer(buffer.host.buffer())
                .regions(&copies);
            device.cmd_copy_buffer2(cmd, &info);
            device.end_command_buffer(cmd)?;
        }

        Ok(cmd)
    }

    pub fn encode_frame(
        &mut self,
        device: &ash::Device,
        extensions: &Extensions,
        video_session: &mut VideoSession,
        image_view: vk::ImageView,
        compute_queue: vk::Queue,
        encode_queue: vk::Queue,
        _wait_semaphore_infos: &[vk::SemaphoreSubmitInfo],
        signal_semaphore_compute: &[vk::SemaphoreSubmitInfo],
        output: Option<&mut impl Write>,
    ) -> anyhow::Result<()> {
        unsafe {
            let cmd = self.compute_cmd_buffers[&(image_view, self.next_image)];
            debug!("encode_frame");

            let buffer = self
                .bitstream_buffers
                .as_mut()
                .map_err(|e| *e)?
                .next(device, 100, output)?;

            let encode_cmd =
                self.record_encode_cmd_buffer(device, extensions, &buffer, video_session)?;
            // TODO: mutex around compute queue
            let cmd_infos = [vk::CommandBufferSubmitInfo::default().command_buffer(cmd)];
            let signal_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_semaphore)
                .value(self.frame_index + 1)
                .stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)]
            .iter()
            .copied()
            .chain(signal_semaphore_compute.iter().copied())
            .collect_vec();
            let info = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_infos)
                //.wait_semaphore_infos(_wait_semaphore_infos)
                .signal_semaphore_infos(&signal_infos);
            device
                .queue_submit2(compute_queue, &[info], vk::Fence::null())
                .map_err(|err| anyhow!("Failed to submit to compute queue: {err}"))?;

            let wait_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_semaphore)
                .value(self.frame_index + 1)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let signal_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.encode_semaphore)
                .value(self.frame_index + 1)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let cmd_infos = [vk::CommandBufferSubmitInfo::default().command_buffer(encode_cmd.cmd)];
            let info = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_infos)
                .wait_semaphore_infos(&wait_infos)
                .signal_semaphore_infos(&signal_infos);
            device
                .queue_submit2(encode_queue, &[info], encode_cmd.fence)
                .map_err(|err| anyhow!("Failed to submit to encode queue: {err}"))?;
        }
        self.next_image += 1;
        if self.next_image as usize >= self.views.len() {
            self.next_image = 0;
        }
        self.frame_index = self.frame_index.wrapping_add(1);
        Ok(())
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        unsafe {
            for view in self.views.drain(..) {
                device.destroy_image_view(view, allocator);
            }
            for view in self.y_views.drain(..) {
                device.destroy_image_view(view, allocator);
            }
            for view in self.uv_views.drain(..) {
                device.destroy_image_view(view, allocator);
            }
            for image in self.images.drain(..) {
                device.destroy_image(image, allocator);
            }
            for memory in self.memory.drain(..) {
                device.free_memory(memory, allocator);
            }
            if let Ok(compute_pipeline) = &mut self.compute_pipeline {
                compute_pipeline.destroy(device, allocator);
            }
            if let Ok(shader) = self.compute_shader.as_mut() {
                shader.destroy(device, allocator);
            }

            device.destroy_descriptor_pool(self.descriptor_pool, allocator);
            device.destroy_command_pool(self.compute_cmd_pool, allocator);
            if let Ok(pool) = &mut self.encode_cmd_pool {
                pool.destroy(device, allocator);
            }
            if let Ok(pool) = &mut self.decode_cmd_pool {
                pool.destroy(device, allocator);
            }
            device.destroy_semaphore(self.compute_semaphore, allocator);
            device.destroy_semaphore(self.encode_semaphore, allocator);
        }
    }
    // TODO: DropBomb?

    pub fn coded_extent(&self) -> vk::Extent2D {
        let vk::Extent2D {
            mut width,
            mut height,
        } = self.extent;
        width = (width + 15) / 16 * 16;
        height = (height + 15) / 16 * 16;
        vk::Extent2D { width, height }
    }
}
