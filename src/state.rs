use std::sync::RwLock;

#[cfg(debug_assertions)]
use ash::ext;
use ash::khr;
use ash::vk;
use once_cell::sync::Lazy;

use crate::settings::Settings;

#[derive(Default)]
pub struct Extensions {
    swapchain_fn: Option<khr::swapchain::DeviceFn>,
    video_queue_fn: Option<khr::video_queue::DeviceFn>,
    video_encode_queue_fn: Option<khr::video_encode_queue::DeviceFn>,
    #[allow(dead_code)]
    video_decode_queue_fn: Option<khr::video_decode_queue::DeviceFn>,
    #[cfg(debug_assertions)]
    debug_utils_fn: Option<ext::debug_utils::DeviceFn>,
}

impl Extensions {
    pub fn swapchain_fn(&self) -> &khr::swapchain::DeviceFn {
        self.swapchain_fn.as_ref().unwrap()
    }

    pub fn video_queue_fn(&self) -> &khr::video_queue::DeviceFn {
        self.video_queue_fn.as_ref().unwrap()
    }

    pub fn video_encode_queue_fn(&self) -> &khr::video_encode_queue::DeviceFn {
        self.video_encode_queue_fn.as_ref().unwrap()
    }

    #[allow(dead_code)]
    pub fn video_decode_queue_fn(&self) -> &khr::video_decode_queue::DeviceFn {
        self.video_decode_queue_fn.as_ref().unwrap()
    }

    #[cfg(debug_assertions)]
    pub fn debug_utils_fn(&self) -> &ext::debug_utils::DeviceFn {
        self.debug_utils_fn.as_ref().unwrap()
    }

    pub fn set_swapchain_fn(&mut self, swapchain_fn: Option<khr::swapchain::DeviceFn>) {
        self.swapchain_fn = swapchain_fn;
    }

    pub fn set_video_queue_fn(&mut self, video_queue_fn: Option<khr::video_queue::DeviceFn>) {
        self.video_queue_fn = video_queue_fn;
    }

    #[cfg(debug_assertions)]
    pub fn set_debug_utils_fn(&mut self, debug_utils_fn: Option<ext::debug_utils::DeviceFn>) {
        self.debug_utils_fn = debug_utils_fn;
    }

    pub fn set_video_encode_queue_fn(
        &mut self,
        video_encode_queue_fn: Option<khr::video_encode_queue::DeviceFn>,
    ) {
        self.video_encode_queue_fn = video_encode_queue_fn;
    }

    #[allow(dead_code)]
    pub fn set_video_decode_queue_fn(
        &mut self,
        video_decode_queue_fn: Option<khr::video_decode_queue::DeviceFn>,
    ) {
        self.video_decode_queue_fn = video_decode_queue_fn;
    }
}

// TODO: either do object wrapping or hash map dispatch. Until then there can only be a single
// instance/device
// or better: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VK_EXT_private_data.html (in core vk1.3)
#[derive(Default)]
pub struct State {
    pub instance: RwLock<Option<ash::Instance>>,
    pub device: RwLock<Option<ash::Device>>,
    pub physical_device: RwLock<Option<vk::PhysicalDevice>>,
    pub extensions: RwLock<Extensions>,
    pub application_name: RwLock<Option<String>>,
    pub instance_get_fn: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>>,
    pub device_get_fn: RwLock<Option<vk::PFN_vkGetDeviceProcAddr>>,
    pub settings: Settings,
    pub compute_queue: RwLock<Option<vk::Queue>>,
    pub compute_queue_family_idx: RwLock<u32>,
    pub graphics_queue_family_idx: RwLock<u32>,
    pub encode_queue: RwLock<Option<vk::Queue>>,
    pub encode_queue_family_idx: RwLock<u32>,
    pub decode_queue: RwLock<Option<vk::Queue>>,
    pub decode_queue_family_idx: RwLock<u32>,
    pub private_slot: RwLock<vk::PrivateDataSlot>,
}

pub fn get_state() -> &'static State {
    static STATE: Lazy<State> = Lazy::new(|| State {
        settings: Settings::new_from_env(),
        ..Default::default()
    });
    &STATE
}

impl State {}
