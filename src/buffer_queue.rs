use core::slice;
use std::io::Write;

use ash::{prelude::VkResult, vk};
use log::{debug, error, warn};

use crate::vulkan_utils::find_memorytype_index;

#[derive(Clone, Copy)]
pub struct BufferPair {
    pub device: Buffer,
    pub host: Buffer,
    pub slot: u32,
    pub query_pool: vk::QueryPool,
}

#[derive(Clone, Copy)]
pub struct Buffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
    //_marker: PhantomData<Box<()>>, //TODO
}

impl Buffer {
    pub fn new(
        device: &ash::Device,
        buffer_create_info: &vk::BufferCreateInfo,
        memory_props: &vk::PhysicalDeviceMemoryProperties,
        memory_property_flags: vk::MemoryPropertyFlags,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        debug!("allocating memory: {:?}", buffer_create_info);
        unsafe {
            let buffer = device.create_buffer(buffer_create_info, allocator)?;
            let size = buffer_create_info.size;
            let mut rtn = Self {
                memory: vk::DeviceMemory::null(),
                buffer,
                size,
            };

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
        }
    }

    pub fn memory(&self) -> vk::DeviceMemory {
        self.memory
    }
}

pub struct BitstreamBufferRing {
    buffers: Vec<Buffer>,
    host_buffers: Vec<Buffer>,
    buffer_generation: Vec<u64>,
    current: usize,
    generation: u64,
    semaphore: vk::Semaphore,
    query_pool: vk::QueryPool,
}

impl BitstreamBufferRing {
    pub fn new(
        device: &ash::Device,
        buffer_create_info: &vk::BufferCreateInfo,
        count: usize,
        memory_props: &vk::PhysicalDeviceMemoryProperties,
        memory_property_flags: vk::MemoryPropertyFlags,
        buffer_result_timeline_semaphore: vk::Semaphore,
        profile_info: &mut vk::VideoProfileInfoKHR,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        let mut rtn = Self {
            buffers: Vec::with_capacity(count),
            host_buffers: Vec::with_capacity(count),
            buffer_generation: vec![0; count],
            semaphore: buffer_result_timeline_semaphore,
            current: 0,
            generation: 0,
            query_pool: vk::QueryPool::null(),
        };
        if buffer_result_timeline_semaphore == vk::Semaphore::null() {
            warn!("Could not create bitstream buffers because no valid timeline semaphore was provided!");
            return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
        }

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
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                allocator,
            )
            .map_err(|e| {
                rtn.destroy(device, allocator);
                error!("Failed to create host buffer: {e}");
                e
            })?;
            rtn.host_buffers.push(buffer);
        }

        let mut encode_info = vk::QueryPoolVideoEncodeFeedbackCreateInfoKHR::default()
            .encode_feedback_flags(
                vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BUFFER_OFFSET
                    | vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BYTES_WRITTEN,
            );
        let info = vk::QueryPoolCreateInfo::default()
            .query_count(count as u32)
            .query_type(vk::QueryType::VIDEO_ENCODE_FEEDBACK_KHR)
            .push_next(&mut encode_info)
            .push_next(profile_info);

        rtn.query_pool = unsafe { device.create_query_pool(&info, allocator) }.map_err(|e| {
            rtn.destroy(device, allocator);
            error!("Failed to create query buffer: {e}");
            e
        })?;
        Ok(rtn)
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        for buffer in self.buffers.drain(..) {
            buffer.destroy(device, allocator);
        }
        for buffer in self.host_buffers.drain(..) {
            buffer.destroy(device, allocator);
        }
        unsafe {
            device.destroy_query_pool(self.query_pool, allocator);
        }
    }

    pub fn next(
        &mut self,
        device: &ash::Device,
        timeout: u64,
        output: Option<&mut impl Write>,
    ) -> VkResult<BufferPair> {
        let buffer = &self.buffers[self.current];

        let host = &self.host_buffers[self.current];
        let semaphores = [self.semaphore];
        let values = [self.buffer_generation[self.current]];
        let info = vk::SemaphoreWaitInfo::default()
            .values(&values)
            .semaphores(&semaphores);
        unsafe {
            device.wait_semaphores(&info, timeout).map_err(|e| {
                let actual_value = device.get_semaphore_counter_value(self.semaphore);
                warn!(
                    "Failed to wait for encode timeline semaphore in bitstream buffer for value {}. Current value {actual_value:?}",
                    values[0]
                );
                e
            })?;
        }

        #[derive(Default, Debug, Copy, Clone)]
        #[repr(C)]
        struct QueryStatus {
            offset: u32,
            size: u32,
            status: vk::QueryResultStatusKHR,
        }
        let mut result = [QueryStatus::default()];

        if values[0] != 0 {
            let slot = ((values[0] as usize - 1) % self.buffer_generation.len()) as u32;
            let result = unsafe {
                let result = device
                    .get_query_pool_results(
                        self.query_pool,
                        slot,
                        &mut result,
                        vk::QueryResultFlags::WAIT | vk::QueryResultFlags::WITH_STATUS_KHR,
                    )
                    .map_err(|e| {
                        warn!(
                            "Failed to get query results for query slot {slot} for encoding {}",
                            values[0]
                        );
                        e
                    })
                    .and_then(|_| Ok(result[0]));
                device.reset_query_pool(self.query_pool, slot, 1);
                if result.is_err()
                    || result.unwrap().status != vk::QueryResultStatusKHR::COMPLETE
                    || result.unwrap().size == 0
                {
                    warn!("{:?} slot {slot} encoding {}", result, values[0]);
                }
                result
            };
            // TODO: offload to IO thread
            if let Ok(result) = result {
                let size = host.size().min(result.size.into());
                unsafe {
                    let data = device.map_memory(
                        host.memory(),
                        result.offset.into(),
                        size,
                        vk::MemoryMapFlags::default(),
                    );
                    if let (Ok(data), Some(output)) = (data, output) {
                        let res = output
                            .write_all(slice::from_raw_parts(data as *const u8, size as usize));
                        debug!("Wrote {}B to output file: {res:?}", size);
                    }
                    let _ = device.unmap_memory(host.memory());
                }
            }
        }

        self.buffer_generation[self.current] = self.generation + 1;
        let rtn = BufferPair {
            device: *buffer,
            host: *host,
            slot: self.current as u32,
            query_pool: self.query_pool,
        };
        self.generation += 1;
        self.current += 1;
        if self.current >= self.buffers.len() {
            self.current = 0;
        }
        Ok(rtn)
    }
}
