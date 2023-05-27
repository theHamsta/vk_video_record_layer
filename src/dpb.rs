use std::{collections::HashMap, mem::MaybeUninit, ptr::null};

use anyhow::anyhow;
use ash::{prelude::VkResult, vk};
use itertools::Itertools;
use log::{debug, error};

use crate::{
    cmd_buffer_queue::{CommandBuffer, CommandBufferQueue},
    settings::Codec,
    shader::ShaderPipeline,
    video_session::VideoSession,
};

pub struct Dpb {
    extent: vk::Extent2D,
    video_format: vk::Format,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    y_views: Vec<vk::ImageView>,
    uv_views: Vec<vk::ImageView>,
    memory: Vec<vk::DeviceMemory>, // TODO: only one memory?
    sampler: vk::Sampler,
    compute_cmd_pool: vk::CommandPool,
    encode_cmd_pool: VkResult<CommandBufferQueue>,
    decode_cmd_pool: VkResult<CommandBufferQueue>,
    descriptor_pool: vk::DescriptorPool,
    next_image: u32,
    compute_family_index: u32,
    encode_family_index: u32,
    decode_family_index: u32,
    compute_cmd_buffers: HashMap<(vk::ImageView, u32), vk::CommandBuffer>,
    sets: Vec<vk::DescriptorSet>,
    compute_pipeline: vk::Pipeline,
    compute_pipeline_layout: vk::PipelineLayout,
    compute_descriptor_layouts: Vec<vk::DescriptorSetLayout>,
    compute_shader: anyhow::Result<ShaderPipeline>,
    compute_semaphore: vk::Semaphore,
    encode_semaphore: vk::Semaphore,
    frame_index: u64,
}

impl Dpb {
    pub fn new(
        // src_queue_family_index
        device: &ash::Device,
        video_format: vk::Format,
        extent: vk::Extent2D,
        num_images: u32,
        max_input_image_views: u32,
        allocator: Option<&vk::AllocationCallbacks>,
        encode_family_index: u32,
        decode_family_index: u32,
        compute_family_index: u32,
        video_session: &VideoSession,
    ) -> VkResult<Self> {
        unsafe {
            let mut images = Vec::new();
            let mut memory = Vec::new();
            let mut views = Vec::new();
            let mut y_views = Vec::new();
            let mut uv_views = Vec::new();
            let vk::Extent2D { width, height } = extent;
            let mut res = vk::Result::SUCCESS;
            let indices = [
                encode_family_index,
                decode_family_index,
                compute_family_index,
            ];

            let profile = vk::VideoProfileInfoKHR::default()
                .video_codec_operation(match (video_session.is_encode(), video_session.codec()) {
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

            let mut info = vk::ImageCreateInfo::default()
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
            let profiles = &[profile];
            let mut profile_list = vk::VideoProfileListInfoKHR::default().profiles(profiles);
            info = info.push_next(&mut profile_list);

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
                .format(video_format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_0,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let mut uv_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(video_format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_1,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });

            for _ in 0..num_images {
                let image = device.create_image(&info, allocator);
                let Ok(image) = image.map_err(|e| { error!("Failed to create image for DPB: {e}"); res = e}) else { break; };
                images.push(image);

                view_info.image = image;
                let view = device.create_image_view(&view_info, allocator);
                let Ok(view) = view.map_err(|e| { error!("Failed to create color image view for DPB: {e}"); res = e}) else { break; };
                views.push(view);

                y_view_info.image = image;
                let view = device.create_image_view(&y_view_info, allocator);
                let Ok(view) = view.map_err(|e| { error!("Failed to create luma image view for DPB: {e}"); res = e}) else { break; };
                y_views.push(view);

                uv_view_info.image = image;
                let view = device.create_image_view(&uv_view_info, allocator);
                let Ok(view) = view.map_err(|e| { error!("Failed to create chroma image view for DPB: {e}"); res = e}) else { break; };
                uv_views.push(view);

                let req = device.get_image_memory_requirements(image);
                let info = vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(req.memory_type_bits.trailing_zeros());
                let Ok(mem) = device.allocate_memory(&info, allocator).map_err(|e| res = e) else {break;}; // TODO: one big allocation
                memory.push(mem);

                if let Err(err) = device.bind_image_memory(image, mem, req.size) {
                    res = err;
                    break;
                }
            }

            let sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .unnormalized_coordinates(true)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER),
                    allocator,
                )
                .map_err(|err| res = err)
                .unwrap_or(vk::Sampler::null());

            let info =
                vk::CommandPoolCreateInfo::default().queue_family_index(compute_family_index);
            let compute_cmd_pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| {
                    error!("Failed to allocate compute command pool: {res}");
                    res = err
                })
                .unwrap_or(vk::CommandPool::null());

            let encode_cmd_pool =
                CommandBufferQueue::new(device, encode_family_index, 10, 100, allocator);
            let decode_cmd_pool =
                CommandBufferQueue::new(device, encode_family_index, 10, 100, allocator);

            let num_pools = max_input_image_views * num_images;
            let pool_sizes = vec![
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_IMAGE)
                    .descriptor_count(2),
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

            let (compute_pipeline, compute_pipeline_layout, compute_descriptor_layouts) =
                if let Ok(shader) = compute_shader.as_ref() {
                    shader
                        .make_compute_pipeline(device, "main", &[], allocator)
                        .unwrap_or_else(|_| {
                            (vk::Pipeline::null(), vk::PipelineLayout::null(), Vec::new())
                        })
                } else {
                    (vk::Pipeline::null(), vk::PipelineLayout::null(), Vec::new())
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

            let mut rtn = Self {
                next_image: 0,
                frame_index: 0,
                extent,
                video_format,
                images,
                views,
                y_views,
                uv_views,
                memory,
                sampler,
                compute_family_index,
                encode_family_index,
                decode_family_index,
                compute_cmd_pool,
                encode_cmd_pool,
                decode_cmd_pool,
                descriptor_pool,
                compute_cmd_buffers: Default::default(),
                compute_pipeline,
                compute_pipeline_layout,
                compute_descriptor_layouts,
                compute_shader,
                compute_semaphore,
                encode_semaphore,
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
        if input_format != vk::Format::B8G8R8A8_UNORM {
            panic!("Conversion for input format {input_format:?} not implemented yet");
        }

        // TODO: transition encode images to shader write
        unsafe {
            let info = vk::CommandBufferAllocateInfo::default()
                .command_pool(self.compute_cmd_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count((input_images.len() * self.views.len()) as u32);
            let mut cmds = device.allocate_command_buffers(&info)?;
            for (&image, &view) in input_images.iter().zip(input_image_views) {
                for i in 0..self.views.len() {
                    let cmd = cmds.pop().unwrap();
                    let barriers = vec![vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_GRAPHICS)
                        .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                        .src_access_mask(vk::AccessFlags2::SHADER_WRITE)
                        .dst_access_mask(vk::AccessFlags2::SHADER_READ)
                        .old_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
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
                        .image(image)];
                    let dep_info_present_to_compute =
                        vk::DependencyInfo::default().image_memory_barriers(&barriers);
                    let barriers = vec![vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                        .src_access_mask(vk::AccessFlags2::SHADER_READ)
                        .dst_access_mask(vk::AccessFlags2::NONE)
                        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
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
                        .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
                    device
                        .begin_command_buffer(cmd, &info)
                        .map_err(|err| anyhow!("Failed to begin command buffer: {err}"))?;
                    device.cmd_pipeline_barrier2(cmd, &dep_info_present_to_compute);
                    let info = vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.descriptor_pool)
                        .set_layouts(&self.compute_descriptor_layouts);

                    let set = device.allocate_descriptor_sets(&info)?;
                    self.sets.push(set[0]);
                    device.update_descriptor_sets(
                        &[
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(0)
                                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                                .image_info(&[vk::DescriptorImageInfo::default()
                                    .image_view(view)
                                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]),
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(1)
                                .descriptor_type(vk::DescriptorType::SAMPLER)
                                .image_info(&[
                                    vk::DescriptorImageInfo::default().sampler(self.sampler)
                                ]),
                            vk::WriteDescriptorSet::default()
                                .dst_set(set[0])
                                .dst_binding(2)
                                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                                .image_info(&[
                                    vk::DescriptorImageInfo::default()
                                        .image_view(self.y_views[i])
                                        .image_layout(vk::ImageLayout::GENERAL),
                                    vk::DescriptorImageInfo::default()
                                        .image_view(self.uv_views[i])
                                        .image_layout(vk::ImageLayout::GENERAL),
                                ]),
                        ],
                        &[],
                    );

                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        self.compute_pipeline_layout,
                        0,
                        &[set[0]],
                        &[0],
                    );
                    device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        self.compute_pipeline,
                    );
                    device.cmd_dispatch(cmd, self.extent.width / 8, self.extent.height / 8, 1);
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
        video_queue_fn: &vk::KhrVideoQueueFn,
        video_encode_queue_fn: &vk::KhrVideoEncodeQueueFn,
        video_session: &VideoSession,
        quality_level: u32,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> anyhow::Result<CommandBuffer> {
        let cmd = self
            .encode_cmd_pool
            .as_mut()
            .map_err(|e| *e)?
            .next(device, allocator)?;
        unsafe {
            let cmd = cmd.cmd;
            let info = vk::VideoBeginCodingInfoKHR::default()
                .video_session(video_session.session())
                .video_session_parameters(
                    video_session
                        .parameters()
                        .ok_or_else(|| anyhow!("Can't encode: missing VideoSessionParameters"))?,
                );
            (video_queue_fn.cmd_begin_video_coding_khr)(cmd, &info);

            let image = self.images[self.next_image as usize];
            let image_view = self.views[self.next_image as usize];

            // TODO: rate control at least once
            //(video_queue_fn.cmd_control_video_coding_khr)(cmd, &info);

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

            // TODO: encode pps and sps. Don't I have to do this myself?

            let buffer = vk::Buffer::null();
            let pic = vk::VideoPictureResourceInfoKHR::default()
                .coded_extent(self.extent)
                .image_view_binding(image_view);
            let flags = MaybeUninit::zeroed();
            let flags = flags.assume_init();
            let h264_pic = vk::native::StdVideoEncodeH264PictureInfo {
                flags,
                seq_parameter_set_id: 0,
                pic_parameter_set_id: 0,
                reserved1: 0,
                frame_num: 0,
                PicOrderCnt: 0,
                pictureType: vk::native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_IDR,
            };
            let flags = MaybeUninit::zeroed();
            let flags = flags.assume_init();
            let h264_header = vk::native::StdVideoEncodeH264SliceHeader {
                flags,
                first_mb_in_slice: 0,
                slice_type: 0,
                idr_pic_id: 0,
                num_ref_idx_l0_active_minus1: 0,
                num_ref_idx_l1_active_minus1: 0,
                cabac_init_idc: 0,
                disable_deblocking_filter_idc: 0,
                slice_alpha_c0_offset_div2: 0,
                slice_beta_offset_div2: 0,
                reserved1: 0,
                reserved2: 0,
                pWeightTable: null(),
            };
            let mb_width = (self.extent.width + 15) / 16;
            let mb_height = (self.extent.height + 15) / 16;
            let h264_nalus = &[vk::VideoEncodeH264NaluSliceInfoEXT::default()
                .std_slice_header(&h264_header)
                .mb_count(mb_width * mb_height)];
            let mut h264_info = vk::VideoEncodeH264VclFrameInfoEXT::default()
                .nalu_slice_entries(h264_nalus)
                .std_picture_info(&h264_pic);
            let mut info = vk::VideoEncodeInfoKHR::default()
                .quality_level(quality_level)
                .dst_buffer(buffer)
                .dst_buffer_range(0)
                .src_picture_resource(pic);
            match video_session.codec() {
                Codec::H264 => info = info.push_next(&mut h264_info),
                Codec::H265 => todo!(),
                Codec::AV1 => todo!(),
            };
            (video_encode_queue_fn.cmd_encode_video_khr)(cmd, &info);

            let info = vk::VideoEndCodingInfoKHR::default();
            (video_queue_fn.cmd_end_video_coding_khr)(cmd, &info);
        }

        Ok(cmd)
    }

    pub fn encode_frame(
        &mut self,
        device: &ash::Device,
        video_queue_fn: &vk::KhrVideoQueueFn,
        video_encode_queue_fn: &vk::KhrVideoEncodeQueueFn,
        video_session: &VideoSession,
        quality_level: u32,
        image_view: vk::ImageView,
        compute_queue: vk::Queue,
        encode_queue: vk::Queue,
        wait_semaphore_infos: &[vk::SemaphoreSubmitInfo],
        signal_semaphore_infos: &[vk::SemaphoreSubmitInfo],
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> anyhow::Result<()> {
        unsafe {
            let cmd = self.compute_cmd_buffers[&(image_view, self.next_image)];
            debug!("encode_frame");

            let encode_cmd = self.record_encode_cmd_buffer(
                device,
                video_queue_fn,
                video_encode_queue_fn,
                video_session,
                quality_level,
                allocator,
            )?;

            // TODO: mutex around compute queue
            let cmd_infos = [vk::CommandBufferSubmitInfo::default().command_buffer(cmd)];
            let signal_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_semaphore)
                .value(self.frame_index)
                .stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)]
            .iter()
            .copied()
            .chain(signal_semaphore_infos.iter().copied())
            .collect_vec();
            let info = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_infos)
                //.wait_semaphore_infos(wait_semaphore_infos)
                .signal_semaphore_infos(&signal_infos);
            device
                .queue_submit2(compute_queue, &[info], vk::Fence::null())
                .map_err(|err| anyhow!("Failed to submit to compute queue: {err}"))?;

            let wait_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_semaphore)
                .value(self.frame_index)
                .stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)];
            let cmd_infos = [vk::CommandBufferSubmitInfo::default().command_buffer(encode_cmd.cmd)];
            let info = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_infos)
                .wait_semaphore_infos(&wait_infos);
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
            for layout in self.compute_descriptor_layouts.drain(..) {
                device.destroy_descriptor_set_layout(layout, allocator);
            }
            if let Ok(shader) = self.compute_shader.as_mut() {
                shader.destroy(device, allocator);
            }

            device.destroy_pipeline(self.compute_pipeline, allocator);
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
}
