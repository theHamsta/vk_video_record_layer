use ash::{prelude::VkResult, vk};
use log::{debug, error};

// From ash examples
fn find_memorytype_index(
    memory_req: &vk::MemoryRequirements,
    memory_prop: &vk::PhysicalDeviceMemoryProperties,
    flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
    memory_prop.memory_types[..memory_prop.memory_type_count as _]
        .iter()
        .enumerate()
        .find(|(index, memory_type)| {
            (1 << index) & memory_req.memory_type_bits != 0
                && memory_type.property_flags & flags == flags
        })
        .map(|(index, _memory_type)| index as _)
}

#[derive(Clone, Copy)]
pub struct BufferPair {
    pub device: Buffer,
    pub host: Buffer,
}

#[derive(Clone, Copy, Default)]
pub enum Fence {
    Fence(vk::Fence),
    #[default]
    Nothing,
}

#[derive(Clone, Copy)]
#[allow(dead_code)] // we don't use fence at the moment
pub enum SyncPrimitive {
    Fence,
    Nothing,
}

#[derive(Clone, Copy)]
pub struct Buffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    fence: Fence,
    size: u64,
    //_marker: PhantomData<Box<()>>, //TODO
}

impl Buffer {
    pub fn new(
        device: &ash::Device,
        buffer_create_info: &vk::BufferCreateInfo,
        memory_props: &vk::PhysicalDeviceMemoryProperties,
        memory_property_flags: vk::MemoryPropertyFlags,
        sync_primitive: SyncPrimitive,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        debug!("allocating memory: {:?}", buffer_create_info);
        unsafe {
            let buffer = device.create_buffer(buffer_create_info, allocator)?;
            let size = buffer_create_info.size;
            let mut rtn = Self {
                memory: vk::DeviceMemory::null(),
                buffer,
                fence: Fence::default(),
                size,
            };
            match sync_primitive {
                SyncPrimitive::Fence => {
                    rtn.fence = Fence::Fence(
                        device
                            .create_fence(
                                &vk::FenceCreateInfo::default()
                                    .flags(vk::FenceCreateFlags::SIGNALED),
                                allocator,
                            )
                            .map_err(|e| {
                                rtn.destroy(device, allocator);
                                error!("Failed create fence: {e}");
                                e
                            })?,
                    );
                }
                SyncPrimitive::Nothing => (),
            }

            let req = device.get_buffer_memory_requirements(buffer);
            let index = find_memorytype_index(&req, memory_props, memory_property_flags)
                .ok_or_else(|| {
                    rtn.destroy(device, allocator);
                    error!("Failed to get memory index");
                    vk::Result::ERROR_INITIALIZATION_FAILED
                })?;

            let info = vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(index);
            let mut flag_info = vk::MemoryAllocateFlagsInfo::default()
                .flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
            let info = info.push_next(&mut flag_info);
            rtn.memory = device.allocate_memory(&info, allocator).map_err(|e| {
                rtn.destroy(device, allocator);
                error!("Failed to allocate memory: {e}");
                e
            })?;

            device
                .bind_buffer_memory(buffer, rtn.memory, 0)
                .map_err(|e| {
                    rtn.destroy(device, allocator);
                    error!("Failed to bind memory: {e}");
                    e
                })?;

            Ok(rtn)
        }
    }

    pub fn buffer(&self) -> vk::Buffer {
        self.buffer
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    fn destroy(self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        unsafe {
            device.destroy_buffer(self.buffer, allocator);
            device.free_memory(self.memory, allocator);
            match self.fence {
                Fence::Fence(fence) => device.destroy_fence(fence, allocator),
                Fence::Nothing => (),
            }
        }
    }
}

pub struct BitstreamBufferRing {
    buffers: Vec<Buffer>,
    host_buffers: Vec<Buffer>,
    current: usize,
}

impl BitstreamBufferRing {
    pub fn new(
        device: &ash::Device,
        buffer_create_info: &vk::BufferCreateInfo,
        count: usize,
        memory_props: &vk::PhysicalDeviceMemoryProperties,
        memory_property_flags: vk::MemoryPropertyFlags,
        sync_primitive: SyncPrimitive,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        let mut rtn = Self {
            buffers: Vec::with_capacity(count),
            host_buffers: Vec::with_capacity(count),
            current: 0,
        };

        let mut host_buffer_create_info = vk::BufferCreateInfo::default()
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .size(buffer_create_info.size)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        host_buffer_create_info.p_queue_family_indices = buffer_create_info.p_queue_family_indices;
        host_buffer_create_info.queue_family_index_count =
            buffer_create_info.queue_family_index_count;
        for _ in 0..count {
            let buffer = Buffer::new(
                device,
                buffer_create_info,
                memory_props,
                memory_property_flags,
                sync_primitive,
                allocator,
            )
            .map_err(|e| {
                rtn.destroy(device, allocator);
                error!("Failed to create buffer: {e}");
                e
            })?;
            rtn.buffers.push(buffer);

            let buffer = Buffer::new(
                device,
                &host_buffer_create_info,
                memory_props,
                vk::MemoryPropertyFlags::HOST_VISIBLE,
                sync_primitive,
                allocator,
            )
            .map_err(|e| {
                rtn.destroy(device, allocator);
                error!("Failed to create host buffer: {e}");
                e
            })?;
            rtn.host_buffers.push(buffer);
        }
        Ok(rtn)
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        for buffer in self.buffers.drain(..) {
            buffer.destroy(device, allocator);
        }
        for buffer in self.host_buffers.drain(..) {
            buffer.destroy(device, allocator);
        }
    }

    pub fn next(&mut self, device: &ash::Device, timeout: u64) -> VkResult<BufferPair> {
        let buffer = &self.buffers[self.current];

        unsafe {
            match buffer.fence {
                Fence::Fence(fence) => {
                    device.wait_for_fences(&[fence], true, timeout)?;
                    device.reset_fences(&[fence])?;
                }
                Fence::Nothing => (),
            }
        }
        let host = &self.host_buffers[self.current];

        self.current += 1;
        if self.current >= self.buffers.len() {
            self.current = 0;
        }
        Ok(BufferPair {
            device: *buffer,
            host: *host,
        })
    }
}
