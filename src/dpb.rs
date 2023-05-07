use ash::{prelude::VkResult, vk};
use log::{debug, error};

pub struct Dpb {
    extent: vk::Extent2D,
    format: vk::Format,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    y_views: Vec<vk::ImageView>,
    uv_views: Vec<vk::ImageView>,
    memory: Vec<vk::DeviceMemory>, // TODO: only one memory?
    sampler: vk::Sampler,
    next_image: u32,
}

impl Dpb {
    pub fn new(
        device: &ash::Device,
        format: vk::Format,
        extent: vk::Extent2D,
        num_images: u32,
        allocator: Option<&vk::AllocationCallbacks>,
        queue_family_indices: &[u32],
    ) -> VkResult<Self> {
        unsafe {
            let mut images = Vec::new();
            let mut memory = Vec::new();
            let mut views = Vec::new();
            let mut y_views = Vec::new();
            let mut uv_views = Vec::new();
            let vk::Extent2D { width, height } = extent;
            let mut res = vk::Result::SUCCESS;
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
                .queue_family_indices(queue_family_indices)
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
        }
    }
    // TODO: DropBomb?
}
