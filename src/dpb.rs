#[cfg(debug_assertions)]
use crate::vulkan_utils::name_object;
use crate::{shader::ComputePipelineDescriptor, vulkan_utils::find_memorytype_index};

#[cfg(feature = "nvpro_sample_gop")]
use crate::gop_gen::{
    VkVideoGopStructure, VkVideoGopStructure_GetFrameType, VkVideoGopStructure_destroy,
    VkVideoGopStructure_new,
};
use anyhow::anyhow;
use ash::{prelude::VkResult, vk};
#[cfg(feature = "nvpro_sample_gop")]
use core::slice;
use itertools::Itertools;
use log::{debug, error, trace};
#[cfg(feature = "nvpro_sample_gop")]
use std::ffi::c_void;
use std::{
    collections::HashMap,
    io::Write,
    marker::PhantomData,
    mem::{transmute, zeroed, MaybeUninit},
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

pub struct CbrOptions {
    pub max_bitrate: u64,
    pub average_bitrate: u64,
    pub frame_rate_numerator: u32,
    pub frame_rate_denominator: u32,
}

#[non_exhaustive]
pub enum RateControlKind {
    Cbr(CbrOptions),
}

impl RateControlKind {
    pub fn as_cbr(&self) -> Option<&CbrOptions> {
        #[allow(irrefutable_let_patterns)]
        if let Self::Cbr(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_cbr_mut(&mut self) -> Option<&mut CbrOptions> {
        #[allow(irrefutable_let_patterns)]
        if let Self::Cbr(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the rate control kind is [`Cbr`].
    ///
    /// [`Cbr`]: RateControlKind::Cbr
    #[must_use]
    #[allow(dead_code)]
    pub fn is_cbr(&self) -> bool {
        matches!(self, Self::Cbr(..))
    }
}

pub struct RateControlOptions {
    pub kind: RateControlKind,
    pub virtual_buffer_size_in_ms: u32,
    pub initial_virtual_buffer_size_in_ms: u32,
    pub quality_level: u32,
}

pub struct Dpb<'dpb> {
    extent: vk::Extent2D,
    coded_extent: vk::Extent2D,
    dpb_images: Vec<vk::Image>,
    dpb_views: Vec<vk::ImageView>,
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
    gop_size: u64,
    #[cfg(feature = "nvpro_sample_gop")]
    nvpro_gop: Option<&'dpb mut VkVideoGopStructure>,
    #[cfg(not(feature = "nvpro_sample_gop"))]
    nvpro_gop: Option<&'dpb PhantomData<i32>>,
    rate_control_options: RateControlOptions,
}

#[derive(Debug, Copy, Clone)]
pub enum PictureType {
    Idr,
    #[allow(dead_code)]
    I,
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

#[allow(dead_code)]
pub struct GopOptions {
    pub use_nvpro: bool,
    pub gop_size: u64,
    pub idr_period: u64,
    pub max_consecutive_b_frames: u64,
    pub last_frame_type: PictureType,
}

impl Dpb<'_> {
    #[cfg(feature = "nvpro_sample_gop")]
    fn display_order_to_dpb_idx(&self, idx: u64) -> u64 {
        // we have a very "sophisticated" DPB management
        idx % self.dpb_images.len() as u64
    }

    pub fn new(
        // src_queue_family_index
        device: &ash::Device,
        extensions: &Extensions,
        video_format: vk::Format,
        extent: vk::Extent2D,
        num_dpb_images: u32,
        num_inflight_images: u32,
        max_input_image_views: u32,
        allocator: Option<&vk::AllocationCallbacks>,
        encode_family_index: u32,
        decode_family_index: u32,
        compute_family_index: u32,
        video_session: &VideoSession,
        physical_memory_props: &vk::PhysicalDeviceMemoryProperties,
        gop_options: GopOptions,
        mut rate_control_options: RateControlOptions,
    ) -> VkResult<Self> {
        unsafe {
            if let Some(cbr) = rate_control_options.kind.as_cbr_mut() {
                if cbr.max_bitrate < cbr.average_bitrate {
                    error!("Invalid settings detected! max_bitrate={} < average_bitrate={}. Setting max_bitrate=average_bitrate", cbr.max_bitrate, cbr.average_bitrate);
                    cbr.max_bitrate = cbr.average_bitrate;
                }
            }

            let mut images = Vec::new();
            let mut dpb_images = Vec::new();
            let mut dpb_views = Vec::new();
            let mut memory = Vec::new();
            let mut views = Vec::new();
            let mut y_views = Vec::new();
            let mut uv_views = Vec::new();
            let vk::Extent2D {
                mut width,
                mut height,
            } = extent;
            match video_session.codec() {
                Codec::H264 => {
                    width = (width + 15) / 16 * 16;
                    height = (height + 15) / 16 * 16;
                }
                Codec::H265 => {
                    width = (width + 31) / 32 * 32;
                    height = (height + 31) / 32 * 32;
                }
                Codec::AV1 => todo!(),
            }
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
            let mut info = info.push_next(&mut profile_list);

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

            for i in 0..(num_inflight_images + num_dpb_images) {
                if i >= num_inflight_images {
                    info.usage = vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR;
                }
                let image = device.create_image(&info, allocator);
                let Ok(image) = image.map_err(|e| {
                    error!("Failed to create image for DPB: {e}");
                    res = e
                }) else {
                    break;
                };
                if i < num_inflight_images {
                    images.push(image);
                } else {
                    dpb_images.push(image);
                }

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
                if i < num_inflight_images {
                    views.push(view);
                    #[cfg(debug_assertions)]
                    name_object(device, extensions, view, &format!("DPB view {i}"));
                } else {
                    dpb_views.push(view);
                    continue;
                }

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

            let num_pools = max_input_image_views * num_inflight_images;
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
                .size(5_000_000)
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
            let coded_extent = vk::Extent2D { width, height };

            #[cfg(feature = "nvpro_sample_gop")]
            let nvpro_gop = gop_options.use_nvpro.then(|| {
                let rtn = VkVideoGopStructure_new(
                    gop_options.gop_size.try_into().unwrap_or(16),
                    gop_options.idr_period.try_into().unwrap_or(16),
                    gop_options
                        .max_consecutive_b_frames
                        .try_into()
                        .unwrap_or(16),
                    1,
                    match gop_options.last_frame_type {
                        PictureType::Idr => {
                            crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_IDR
                        }
                        PictureType::I => {
                            crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_I
                        }
                        PictureType::P => {
                            crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_P
                        }
                        PictureType::B => {
                            crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_B
                        }
                    },
                );

                let rtn = rtn
                    .as_mut()
                    .expect("Failed to allocate VkVideoGopStructure");
                rtn.Init();
                rtn
            });
            let nvpro_gop = None;
            let mut rtn = Self {
                next_image: 0,
                frame_index: 0,
                extent,
                coded_extent,
                dpb_images,
                dpb_views,
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
                gop_size: gop_options.gop_size,
                sets: Default::default(),
                nvpro_gop,
                rate_control_options,
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
                                        vk::ImageAspectFlags::COLOR, // color means both planes, the
                                                                     // following is not allowed
                                                                     //vk::ImageAspectFlags::PLANE_0
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
                        &transmute::<ash::vk::Extent2D, [u8; 8]>(self.extent),
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
    ) -> anyhow::Result<(CommandBuffer, u64)> {
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
            //TODO:
            //if self.frame_index < self.dpb_views.len() as u64 {
            //barriers.push(
            //vk::ImageMemoryBarrier2::default()
            //.src_stage_mask(vk::PipelineStageFlags2::NONE)
            //.dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            //.src_access_mask(vk::AccessFlags2::NONE)
            //.dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR)
            //.old_layout(vk::ImageLayout::UNDEFINED)
            //.new_layout(vk::ImageLayout::VIDEO_ENCODE_DPB_KHR)
            //.src_queue_family_index(self.encode_family_index)
            //.dst_queue_family_index(self.encode_family_index)
            //.subresource_range(
            //vk::ImageSubresourceRange::default()
            //.aspect_mask(vk::ImageAspectFlags::COLOR)
            //.base_mip_level(0)
            //.level_count(1)
            //.base_array_layer(0)
            //.layer_count(1),
            //)
            //.image(self.dpb_images[0]),
            //)
            //}
            let info = vk::DependencyInfo::default().image_memory_barriers(&barriers);
            device.cmd_pipeline_barrier2(cmd, &info);

            let image_type = if let Some(_gop) = self.nvpro_gop.as_mut() {
                #[cfg(feature = "nvpro_sample_gop")]
                {
                    let gop = _gop;
                    let first_frame = self.frame_index == 0;
                    let last_frame = false;
                    let pic_type = VkVideoGopStructure_GetFrameType(
                        *gop as *mut VkVideoGopStructure as *mut c_void,
                        self.frame_index,
                        first_frame,
                        last_frame,
                    );
                    match pic_type {
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_P => {
                            PictureType::P
                        }
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_B => {
                            PictureType::B
                        }
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_I => {
                            PictureType::I
                        }
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_IDR => {
                            PictureType::Idr
                        }
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_INTRA_REFRESH => {
                            panic!("Intra refresh not yet supported");
                        }
                        crate::gop_gen::VkVideoGopStructure_FrameType::FRAME_TYPE_INVALID => {
                            panic!("Obtained FRAME_TYPE_INVALID from NVPRO GOP structure");
                        }
                    }
                }
                #[cfg(not(feature = "nvpro_sample_gop"))]
                unreachable!()
            } else {
                if video_session.needs_reset() || self.frame_index % self.gop_size == 0 {
                    PictureType::Idr
                } else {
                    PictureType::P
                }
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
                let mut info = vk::VideoCodingControlInfoKHR::default().flags(
                    vk::VideoCodingControlFlagsKHR::ENCODE_RATE_CONTROL
                        | vk::VideoCodingControlFlagsKHR::ENCODE_QUALITY_LEVEL
                        | vk::VideoCodingControlFlagsKHR::RESET,
                );
                #[cfg(feature = "nvpro_sample_gop")]
                let consecutive_b_frame_count = self
                    .nvpro_gop
                    .as_ref()
                    .map(|gop| gop.m_consecutiveBFrameCount as u32)
                    .unwrap_or(0u32);
                #[cfg(feature = "nvpro_sample_gop")]
                debug_assert_eq!(
                    self.nvpro_gop
                        .as_ref()
                        .map(|gop| gop.m_temporalLayerCount)
                        .unwrap_or(1),
                    1
                );
                #[cfg(not(feature = "nvpro_sample_gop"))]
                let consecutive_b_frame_count = 0;

                let mut encode_control_h264 = vk::VideoEncodeH264RateControlInfoKHR::default()
                    .flags(vk::VideoEncodeH264RateControlFlagsKHR::REGULAR_GOP)
                    .consecutive_b_frame_count(consecutive_b_frame_count)
                    .temporal_layer_count(1)
                    .gop_frame_count(self.gop_size as u32)
                    .idr_period(self.gop_size as u32);
                let mut encode_control_h265 = vk::VideoEncodeH265RateControlInfoKHR::default()
                    .flags(vk::VideoEncodeH265RateControlFlagsKHR::REGULAR_GOP)
                    .consecutive_b_frame_count(consecutive_b_frame_count)
                    .sub_layer_count(1)
                    .gop_frame_count(self.gop_size as u32)
                    .idr_period(self.gop_size as u32);
                let average_bitrate = self
                    .rate_control_options
                    .kind
                    .as_cbr()
                    .map(|cbr| cbr.average_bitrate)
                    .unwrap_or(0);
                let max_bitrate = self
                    .rate_control_options
                    .kind
                    .as_cbr()
                    .map(|cbr| cbr.max_bitrate)
                    .unwrap_or(0);
                let layers = [vk::VideoEncodeRateControlLayerInfoKHR::default()
                    .average_bitrate(average_bitrate)
                    .max_bitrate(max_bitrate)
                    .frame_rate_numerator(
                        self.rate_control_options
                            .kind
                            .as_cbr()
                            .map(|cbr| cbr.frame_rate_numerator)
                            .unwrap_or(60),
                    )
                    .frame_rate_denominator(
                        self.rate_control_options
                            .kind
                            .as_cbr()
                            .map(|cbr| cbr.frame_rate_denominator)
                            .unwrap_or(1),
                    )];
                let mut h264_layers = [vk::VideoEncodeH264RateControlLayerInfoKHR::default()];
                let mut h265_layers = [vk::VideoEncodeH265RateControlLayerInfoKHR::default()];

                let layers: Vec<_> = match video_session.codec() {
                    Codec::H264 => layers
                        .iter()
                        .zip(h264_layers.iter_mut())
                        .map(|(l, l2)| l.push_next(l2))
                        .collect(),
                    Codec::H265 => layers
                        .iter()
                        .zip(h265_layers.iter_mut())
                        .map(|(l, l2)| l.push_next(l2))
                        .collect(),
                    Codec::AV1 => todo!(),
                };
                let mut encode_control = vk::VideoEncodeRateControlInfoKHR::default()
                    .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::CBR)
                    .virtual_buffer_size_in_ms(self.rate_control_options.virtual_buffer_size_in_ms)
                    .initial_virtual_buffer_size_in_ms(
                        self.rate_control_options.initial_virtual_buffer_size_in_ms,
                    )
                    .layers(&layers);

                let mut quality = vk::VideoEncodeQualityLevelInfoKHR::default()
                    .quality_level(self.rate_control_options.quality_level);
                info = info.push_next(&mut encode_control);
                info = info.push_next(&mut quality);
                info = match video_session.codec() {
                    Codec::H264 => info.push_next(&mut encode_control_h264),
                    Codec::H265 => info.push_next(&mut encode_control_h265),
                    Codec::AV1 => todo!(),
                };
                (video_queue_fn.cmd_control_video_coding_khr)(cmd, &info);
                video_session.set_needs_reset(false);
            }

            device.cmd_begin_query(
                cmd,
                buffer.query_pool,
                buffer.slot,
                vk::QueryControlFlags::default(),
            );
            #[cfg(feature = "nvpro_sample_gop")]
            const MAX_REFERENCES: usize = 8; // that's fine even for AV1
            #[cfg(feature = "nvpro_sample_gop")]
            let mut nvpro_references = MaybeUninit::<[i8; MAX_REFERENCES]>::zeroed();
            #[cfg(feature = "nvpro_sample_gop")]
            let mut num_nvpro_references = 0;
            #[cfg(feature = "nvpro_sample_gop")]
            let gop_idx = self.frame_index % self.gop_size;
            #[cfg(feature = "nvpro_sample_gop")]
            let gop_counter = self.frame_index / self.gop_size;
            #[cfg(feature = "nvpro_sample_gop")]
            if let Some(gop) = &self.nvpro_gop {
                num_nvpro_references = gop.GetReferenceNumbers_c_signature(
                    (self.frame_index % self.gop_size).try_into().unwrap_or(0),
                    nvpro_references.as_mut_ptr() as *mut i8,
                    MAX_REFERENCES,
                    true,
                    true,
                );
            }
            #[cfg(feature = "nvpro_sample_gop")]
            let nvpro_references = nvpro_references.assume_init();
            #[cfg(feature = "nvpro_sample_gop")]
            let nvpro_references =
                slice::from_raw_parts(nvpro_references.as_ptr(), num_nvpro_references as usize);
            #[cfg(feature = "nvpro_sample_gop")]
            let num_back_refs = nvpro_references
                .iter()
                .filter(|&&i| (i as i64) < (gop_idx as i64))
                .count();
            #[cfg(feature = "nvpro_sample_gop")]
            let num_forward_refs = nvpro_references
                .iter()
                .filter(|&&i| (i as i64) > (gop_idx as i64))
                .count();
            #[cfg(feature = "nvpro_sample_gop")]
            let lowest_idx = nvpro_references.iter().copied().min().unwrap_or(0);
            #[cfg(feature = "nvpro_sample_gop")]
            let highest_idx = nvpro_references
                .iter()
                .copied()
                .min()
                .unwrap_or(gop_idx as i8);
            #[cfg(feature = "nvpro_sample_gop")]
            if self.nvpro_gop.is_some() {
                debug!(
                    "NVPRO suggested the following references: {:?} for frame {}",
                    &nvpro_references, self.frame_index
                );
            }
            #[cfg(feature = "nvpro_sample_gop")]
            let dpb_indices: Vec<_> = nvpro_references
                .iter()
                .map(|gop_idx| self.display_order_to_dpb_idx(gop_counter + *gop_idx as u64))
                .collect();
            #[cfg(feature = "nvpro_sample_gop")]
            let dpb_back_indices: Vec<_> = nvpro_references
                .iter()
                .copied()
                .map(|ref_idx| ref_idx as u64)
                .filter(|ref_idx| *ref_idx < gop_idx)
                .map(|ref_idx| {
                    (
                        ref_idx,
                        self.display_order_to_dpb_idx(gop_counter + ref_idx),
                    )
                })
                .collect();
            #[cfg(feature = "nvpro_sample_gop")]
            let dpb_forward_indices: Vec<_> = nvpro_references
                .iter()
                .copied()
                .map(|ref_idx| ref_idx as u64)
                .filter(|ref_idx| *ref_idx > gop_idx)
                .map(|ref_idx| {
                    (
                        ref_idx,
                        self.display_order_to_dpb_idx(gop_counter + ref_idx),
                    )
                })
                .collect();

            let pic = vk::VideoPictureResourceInfoKHR::default()
                .coded_extent(self.coded_extent())
                .image_view_binding(image_view);
            let flags = MaybeUninit::zeroed();
            let mut flags: vk::native::StdVideoEncodeH264PictureInfoFlags = flags.assume_init();
            flags.set_IdrPicFlag(image_type.is_idr() as u32);
            flags.set_is_reference(1);
            let mut ref_lists = vk::native::StdVideoEncodeH264ReferenceListsInfo {
                flags: zeroed(), // set reorder flags
                #[cfg(feature = "nvpro_sample_gop")]
                num_ref_idx_l0_active_minus1: (gop_idx as i8 - lowest_idx as i8 - 1).max(0) as u8,
                #[cfg(feature = "nvpro_sample_gop")]
                num_ref_idx_l1_active_minus1: (highest_idx as i8 - gop_idx as i8 - 1).max(0) as u8,
                #[cfg(not(feature = "nvpro_sample_gop"))]
                num_ref_idx_l0_active_minus1: 0,
                #[cfg(not(feature = "nvpro_sample_gop"))]
                num_ref_idx_l1_active_minus1: 0,
                RefPicList0: [0; 32],
                RefPicList1: [0; 32],
                refList0ModOpCount: 0,
                refList1ModOpCount: 0,
                refPicMarkingOpCount: 0,
                reserved1: Default::default(),
                pRefList0ModOperations: null(),
                pRefList1ModOperations: null(),
                pRefPicMarkingOperations: null(),
            };
            if self.nvpro_gop.is_some() {
                #[cfg(feature = "nvpro_sample_gop")]
                {
                    for &(ref_idx, dpb_idx) in dpb_back_indices.iter() {
                        ref_lists.RefPicList0[(gop_idx - ref_idx - 1) as usize] = dpb_idx as u8;
                    }
                    for &(ref_idx, dpb_idx) in dpb_forward_indices.iter() {
                        ref_lists.RefPicList0[(ref_idx - gop_idx - 1) as usize] = dpb_idx as u8;
                    }
                }
            } else if image_type.is_p() {
                ref_lists.RefPicList0[0] =
                    (self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32) as u8;
                ref_lists.RefPicList0[1] =
                    (self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32) as u8;
            }
            let h264_pic = vk::native::StdVideoEncodeH264PictureInfo {
                flags,
                seq_parameter_set_id: 0,
                pic_parameter_set_id: 0,
                reserved1: [0; 3],
                frame_num: (self.frame_index % self.gop_size) as u32,
                PicOrderCnt: 2 * (self.frame_index % self.gop_size) as i32,
                idr_pic_id: 0,
                temporal_id: 0,
                primary_pic_type: image_type.as_h264_picture_type(),
                pRefLists: &ref_lists,
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
                &[vk::VideoEncodeH264NaluSliceInfoKHR::default().std_slice_header(&h264_header)];
            let mut h264_info = vk::VideoEncodeH264PictureInfoKHR::default()
                .nalu_slice_entries(h264_nalus)
                .std_picture_info(&h264_pic);

            let mut ref_lists = vk::native::StdVideoEncodeH265ReferenceListsInfo {
                flags: zeroed(), // set reorder flags

                #[cfg(feature = "nvpro_sample_gop")]
                num_ref_idx_l0_active_minus1: (dpb_back_indices.len() as i64 - 1).max(0) as u8,
                #[cfg(feature = "nvpro_sample_gop")]
                num_ref_idx_l1_active_minus1: (dpb_forward_indices.len() as i64 - 1).max(0) as u8,
                #[cfg(not(feature = "nvpro_sample_gop"))]
                num_ref_idx_l0_active_minus1: 0,
                #[cfg(not(feature = "nvpro_sample_gop"))]
                num_ref_idx_l1_active_minus1: 0,
                RefPicList0: [0; 15],
                RefPicList1: [0; 15],
                list_entry_l0: [0; 15],
                list_entry_l1: [0; 15],
            };
            if self.nvpro_gop.is_some() {
                #[cfg(feature = "nvpro_sample_gop")]
                {
                    for (i, &(ref_idx, dpb_idx)) in dpb_back_indices.iter().enumerate() {
                        // or vice-versa
                        ref_lists.RefPicList0[i] = dpb_idx as u8;
                        ref_lists.list_entry_l0[i] = (gop_idx - ref_idx - 1) as u8;
                    }
                    for (i, &(ref_idx, dpb_idx)) in dpb_forward_indices.iter().enumerate() {
                        ref_lists.RefPicList1[i] = dpb_idx as u8;
                        ref_lists.list_entry_l1[i] = (ref_idx - gop_idx - 1) as u8;
                    }
                }
            } else if image_type.is_p() {
                ref_lists.RefPicList0[0] =
                    (self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32) as u8;
                ref_lists.RefPicList1[1] =
                    (self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32) as u8;
            }
            let mut flags: vk::native::StdVideoH265ShortTermRefPicSetFlags = zeroed();
            flags.set_inter_ref_pic_set_prediction_flag(1);
            let flags = MaybeUninit::zeroed();
            let mut flags: vk::native::StdVideoEncodeH265PictureInfoFlags = flags.assume_init();
            if image_type.is_p() {
                flags.set_short_term_ref_pic_set_sps_flag(1);
            }
            if self.frame_index != self.gop_size - 1 {
                flags.set_is_reference(1);
            }
            let h265_pic = vk::native::StdVideoEncodeH265PictureInfo {
                flags,
                reserved1: Default::default(),
                pRefLists: &ref_lists,
                pic_type: image_type.as_h265_picture_type(),
                sps_video_parameter_set_id: 0,
                pps_seq_parameter_set_id: 0,
                pps_pic_parameter_set_id: 0,
                short_term_ref_pic_set_idx: 0, // which short term RPS to use (sps with short_term_ref_pic_set_sps_flag or set here if flag not set)
                PicOrderCntVal: (self.frame_index % self.gop_size) as i32,
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
                MaxNumMergeCand: 5,
                slice_cb_qp_offset: 0,
                slice_cr_qp_offset: 0,
                slice_tc_offset_div2: 0,
                slice_act_y_qp_offset: 0,
                slice_act_cb_qp_offset: 0,
                slice_act_cr_qp_offset: 0,
            };
            let h265_nalus = &[vk::VideoEncodeH265NaluSliceSegmentInfoKHR::default()
                .std_slice_segment_header(&h265_header)];
            let mut h265_info = vk::VideoEncodeH265PictureInfoKHR::default()
                .nalu_slice_segment_entries(h265_nalus)
                .std_picture_info(&h265_pic);

            let mut reference_slots = Vec::new();
            let real_h264_setup_info = vk::native::StdVideoEncodeH264ReferenceInfo {
                flags: zeroed(),
                FrameNum: if self.frame_index % self.gop_size != 0 {
                    (self.frame_index % self.gop_size) as u32 - 1
                } else {
                    0
                },
                primary_pic_type: vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_P, // not always correct
                PicOrderCnt: 2 * if self.frame_index % self.gop_size != 0 {
                    (self.frame_index % self.gop_size) as i32 - 1
                } else {
                    0
                },
                long_term_pic_num: 0,
                long_term_frame_idx: 0,
                temporal_id: 0,
            };
            let mut h264_reference_info = vk::VideoEncodeH264DpbSlotInfoKHR::default()
                .std_reference_info(&real_h264_setup_info);
            let flags: vk::native::StdVideoEncodeH265ReferenceInfoFlags = zeroed();
            //flags.set_unused_for_reference(1);
            let real_h265_setup_info = vk::native::StdVideoEncodeH265ReferenceInfo {
                flags,
                pic_type: vk::native::StdVideoH265PictureType_STD_VIDEO_H265_PICTURE_TYPE_P,
                PicOrderCntVal: if self.frame_index % self.gop_size != 0 {
                    (self.frame_index % self.gop_size) as i32 - 1
                } else {
                    0
                },
                TemporalId: 0,
            };
            let mut h265_reference_info = vk::VideoEncodeH265DpbSlotInfoKHR::default()
                .std_reference_info(&real_h265_setup_info);
            let ref_pic_res = vk::VideoPictureResourceInfoKHR::default()
                .coded_extent(self.coded_extent())
                .image_view_binding(
                    self.dpb_views[((self.frame_index + self.dpb_views.len() as u64 - 1)
                        % self.dpb_views.len() as u64) as usize],
                );
            if image_type.is_p() {
                let slot_index =
                    (self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32);
                let mut info = vk::VideoReferenceSlotInfoKHR::default()
                    .slot_index(slot_index)
                    .picture_resource(&ref_pic_res);
                match video_session.codec() {
                    Codec::H264 => info = info.push_next(&mut h264_reference_info),
                    Codec::H265 => info = info.push_next(&mut h265_reference_info),
                    Codec::AV1 => todo!(),
                };
                reference_slots.push(info);
            }
            let setup_pic_res = vk::VideoPictureResourceInfoKHR::default()
                .coded_extent(self.coded_extent())
                .image_view_binding(
                    self.dpb_views[self.frame_index as usize % self.dpb_views.len()],
                );
            let ref_slot_info = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index((self.frame_index as i32 - 1).rem_euclid(self.dpb_views.len() as i32))
                .picture_resource(&setup_pic_res);
            let mut info = vk::VideoEncodeInfoKHR::default()
                .dst_buffer(buffer.device.buffer())
                .dst_buffer_range(buffer.device.size())
                .src_picture_resource(pic)
                .reference_slots(&reference_slots)
                .setup_reference_slot(&ref_slot_info);

            match video_session.codec() {
                Codec::H264 => info = info.push_next(&mut h264_info),
                Codec::H265 => info = info.push_next(&mut h265_info),
                Codec::AV1 => todo!(),
            };
            (video_encode_queue_fn.cmd_encode_video_khr)(cmd, &info);
            device.cmd_end_query(cmd, buffer.query_pool, buffer.slot);

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
            debug!("ende cmd buffer");
        }

        #[cfg(not(feature = "nvpro_sample_gop"))]
        let decode_order_idx = self.frame_index;
        #[cfg(feature = "nvpro_sample_gop")]
        let decode_order_idx = if let Some(gop) = &mut self.nvpro_gop {
            unsafe { gop.GetFrameInDecodeOrder(self.frame_index) }
        } else {
            self.frame_index
        };
        trace!("Recorded encode command buffer");
        Ok((cmd, decode_order_idx))
    }

    pub fn encode_frame(
        &mut self,
        device: &ash::Device,
        extensions: &Extensions,
        video_session: &mut VideoSession,
        image_view: vk::ImageView,
        compute_queue: vk::Queue,
        encode_queue: vk::Queue,
        wait_semaphore_infos: &[vk::SemaphoreSubmitInfo],
        signal_semaphore_compute: &[vk::SemaphoreSubmitInfo],
        output: Option<&mut impl Write>,
    ) -> anyhow::Result<()> {
        unsafe {
            let cmd = self.compute_cmd_buffers[&(image_view, self.next_image)];
            debug!("encode_frame");

            let buffer = self
                .bitstream_buffers
                .as_mut()
                .map_err(|e| {
                    error!("failed to acquire bitstream_buffers");
                    *e
                })?
                .next(device, 100, output)
                .map_err(|err| anyhow!("Failed to next: {err}"))?;

            let (encode_cmd, decode_order_idx) =
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
                .wait_semaphore_infos(wait_semaphore_infos)
                .signal_semaphore_infos(&signal_infos);
            device
                .queue_submit2(compute_queue, &[info], vk::Fence::null())
                .map_err(|err| anyhow!("Failed to submit to compute queue: {err}"))?;

            let wait_infos = [
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.compute_semaphore)
                    .value(self.frame_index + 1)
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.encode_semaphore)
                    .value(decode_order_idx)
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
            ];
            let signal_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.encode_semaphore)
                .value(decode_order_idx + 1)
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
            for view in self.dpb_views.drain(..) {
                device.destroy_image_view(view, allocator);
            }
            for image in self.images.drain(..) {
                device.destroy_image(image, allocator);
            }
            for image in self.dpb_images.drain(..) {
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
            #[cfg(feature = "nvpro_sample_gop")]
            if let Some(gop) = self.nvpro_gop.as_mut() {
                VkVideoGopStructure_destroy(*gop as *mut VkVideoGopStructure);
            }
        }
    }
    // TODO: DropBomb?

    pub fn coded_extent(&self) -> vk::Extent2D {
        self.coded_extent
    }
}
