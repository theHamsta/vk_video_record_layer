use std::collections::HashMap;

use ash::{prelude::VkResult, vk};
use log::{debug, error};

use crate::shader::ShaderPipeline;

pub struct Dpb {
    extent: vk::Extent2D,
    format: vk::Format,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    y_views: Vec<vk::ImageView>,
    uv_views: Vec<vk::ImageView>,
    memory: Vec<vk::DeviceMemory>, // TODO: only one memory?
    sampler: vk::Sampler,
    compute_cmd_pool: vk::CommandPool,
    encode_cmd_pool: vk::CommandPool,
    decode_cmd_pool: vk::CommandPool,
    descriptor_pool: vk::DescriptorPool,
    next_image: u32,
    compute_family_index: u32,
    encode_family_index: u32,
    decode_family_index: u32,
    compute_cmd_buffers: HashMap<(vk::ImageView, u32), vk::CommandBuffer>,
    sets: Vec<vk::DescriptorSet>,
    compute_pipeline: vk::Pipeline,
    compute_descriptor_layouts: Vec<vk::DescriptorSetLayout>,
    compute_shader: anyhow::Result<ShaderPipeline>,
}

impl Dpb {
    pub fn new(
        // src_queue_family_index
        device: &ash::Device,
        format: vk::Format,
        extent: vk::Extent2D,
        num_images: u32,
        max_input_image_views: u32,
        allocator: Option<&vk::AllocationCallbacks>,
        encode_family_index: u32,
        decode_family_index: u32,
        compute_family_index: u32,
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
            let info = vk::ImageCreateInfo::default()
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .format(format)
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .queue_family_indices(&indices)
                .usage(
                    vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR
                        | vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR
                        | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR
                        | vk::ImageUsageFlags::STORAGE,
                )
                .tiling(vk::ImageTiling::OPTIMAL)
                .initial_layout(vk::ImageLayout::GENERAL);

            let mut view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let mut y_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_0,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let mut uv_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::PLANE_1,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });

            for _ in 0..num_images {
                let image = device.create_image(&info, allocator);
                let Ok(image) = image.map_err(|e| res = e) else { break; };
                images.push(image);

                view_info.image = image;
                let view = device.create_image_view(&view_info, allocator);
                let Ok(view) = view.map_err(|e| res = e) else { break; };
                views.push(view);

                y_view_info.image = image;
                let view = device.create_image_view(&y_view_info, allocator);
                let Ok(view) = view.map_err(|e| res = e) else { break; };
                y_views.push(view);

                uv_view_info.image = image;
                let view = device.create_image_view(&uv_view_info, allocator);
                let Ok(view) = view.map_err(|e| res = e) else { break; };
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

            let sampler = device.create_sampler(
                &vk::SamplerCreateInfo::default().unnormalized_coordinates(true),
                allocator,
            )?;

            let info =
                vk::CommandPoolCreateInfo::default().queue_family_index(compute_family_index);
            let compute_cmd_pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| res = err)
                .unwrap_or(vk::CommandPool::null());
            let info = vk::CommandPoolCreateInfo::default()
                .queue_family_index(encode_family_index)
                .flags(vk::CommandPoolCreateFlags::TRANSIENT);

            let encode_cmd_pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| res = err)
                .unwrap_or(vk::CommandPool::null());
            let decode_cmd_pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| res = err)
                .unwrap_or(vk::CommandPool::null());

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

            let (compute_pipeline, compute_descriptor_layouts) =
                if let Ok(shader) = compute_shader.as_ref() {
                    shader
                        .make_compute_pipeline(device, "main", &[], allocator)
                        .unwrap_or_else(|_| (vk::Pipeline::null(), Vec::new()))
                } else {
                    (vk::Pipeline::null(), Vec::new())
                };

            let mut rtn = Self {
                next_image: 0,
                extent,
                format,
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
                compute_descriptor_layouts,
                compute_shader,
                sets: Default::default(),
            };

            if res == vk::Result::SUCCESS {
                debug!("DPB resource successfully created!");
                Ok(rtn)
            } else {
                error!("Failed to create DPB resources created!");
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
                    device.cmd_pipeline_barrier2(cmd, &dep_info_present_to_compute);
                    let info = vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.descriptor_pool)
                        .set_layouts(&self.compute_descriptor_layouts);

                    let set = device.allocate_descriptor_sets(&info)?;
                    self.sets.push(set[0]);

                    //device.cmd_dispatch();
                    device.cmd_pipeline_barrier2(cmd, &dep_info_compute_to_present);

                    self.compute_cmd_buffers.insert((view, i as u32), cmd);
                }
            }
        }
        Ok(())
    }

    pub fn encode_frame(
        &mut self,
        device: &ash::Device,
        image: vk::Image,
        format: vk::Format,
        src_queue_family_index: u32,
        dst_queue_family_index: u32,
    ) -> anyhow::Result<()> {
        let descriptor_set: vk::DescriptorSet;
        let descriptor_pool: vk::DescriptorPool;
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
            device.destroy_command_pool(self.encode_cmd_pool, allocator);
            device.destroy_command_pool(self.decode_cmd_pool, allocator);
        }
    }
    // TODO: DropBomb?
}
