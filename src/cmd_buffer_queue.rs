use ash::{prelude::VkResult, vk};
use log::error;

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
        queue_family_index: u32,
        queue_length: u32,
        timeout: u64,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<Self> {
        let mut res = vk::Result::SUCCESS;
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
            let Ok(pool) = device
                .create_command_pool(&info, allocator)
                .map_err(|err| {
                    error!("Failed to create command pool");
                    res = err
                }) else {
                return Err(res);
            };
            rtn.pool = pool;

            let info = vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(queue_length);
            let Ok(cmds) = device.allocate_command_buffers(&info).map_err(|e| {
                error!("Failed to allocate command buffers for queue family index {queue_family_index}");
                res = e
            }) else {
                rtn.destroy(device, allocator);
                return Err(res);
            };
            rtn.cmds = cmds;

            Ok(rtn)
        }
    }

    pub fn next(
        &mut self,
        device: &ash::Device,
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> VkResult<CommandBuffer> {
        unsafe {
            let fence = if let Some(fence) = self.fences.get(self.current).copied() {
                device.wait_for_fences(&[fence], true, self.timeout)?;
                device.reset_fences(&[fence])?;
                fence
            } else {
                let info = vk::FenceCreateInfo::default();
                let fence = device.create_fence(&info, allocator)?;
                self.fences.push(fence);
                fence
            };

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
