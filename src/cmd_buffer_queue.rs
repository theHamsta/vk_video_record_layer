use ash::{prelude::VkResult, vk};
use log::error;

use crate::{state::Extensions, vulkan_utils::name_object};

pub struct CommandBuffer {
    pub cmd: vk::CommandBuffer,
    pub fence: vk::Fence,
}

pub struct CommandBufferQueue {
    cmds: Vec<vk::CommandBuffer>,
    fences: Vec<vk::Fence>,
    pool: vk::CommandPool,
    current: usize,
    timeout: u64,
}

impl CommandBufferQueue {
    pub fn new(
        device: &ash::Device,
        extensions: &Extensions,
        queue_family_index: u32,
        queue_length: u32,
        timeout: u64,
        debug_name: &str,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        let mut rtn = Self {
            pool: vk::CommandPool::null(),
            current: 0,
            timeout,
            cmds: Vec::with_capacity(queue_length as usize),
            fences: Vec::with_capacity(queue_length as usize),
        };

        unsafe {
            let info = vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family_index)
                .flags(
                    vk::CommandPoolCreateFlags::TRANSIENT
                        | vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
                );
            let pool = device
                .create_command_pool(&info, allocator)
                .map_err(|err| {
                    error!("Failed to create command pool");
                    err
                })?;
            rtn.pool = pool;

            #[cfg(debug_assertions)]
            name_object(device, extensions, pool, debug_name);

            let info = vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(queue_length);
            let cmds = device.allocate_command_buffers(&info).map_err(|e| {
                error!("Failed to allocate command buffers for queue family index {queue_family_index}");
                rtn.destroy(device, allocator);
                e
            })?;
            rtn.cmds = cmds;

            for i in 0..queue_length {
                #[cfg(debug_assertions)]
                name_object(
                    device,
                    extensions,
                    rtn.cmds[i as usize],
                    &format!("{debug_name} {i}"),
                );

                let info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
                let fence = device.create_fence(&info, allocator).map_err(|e| {
                    error!("Failed to create fence: {e}");
                    rtn.destroy(device, allocator);
                    e
                })?;
                rtn.fences.push(fence);
            }

            drop(debug_name);
            drop(extensions);

            Ok(rtn)
        }
    }

    pub fn next(&mut self, device: &ash::Device) -> VkResult<CommandBuffer> {
        unsafe {
            let fence = self.fences[self.current];
            device.wait_for_fences(&[fence], true, self.timeout)?;
            device.reset_fences(&[fence])?;

            self.current += 1;
            if self.current >= self.cmds.len() {
                self.current = 0;
            }

            let cmd = self.cmds[self.current];
            device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::default())?;

            Ok(CommandBuffer { cmd, fence })
        }
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        unsafe {
            for fence in self.fences.drain(..) {
                device.destroy_fence(fence, allocator);
            }
            device.free_command_buffers(self.pool, &self.cmds);
            device.destroy_command_pool(self.pool, allocator);
            self.cmds.clear();
        }
    }
}
