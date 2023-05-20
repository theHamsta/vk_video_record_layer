use std::sync::RwLock;

use ash::vk;
use once_cell::sync::Lazy;

use crate::settings::Settings;

// TODO: either do object wrapping or hash map dispatch. Until then there can only be a single
// instance/device
// or better: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VK_EXT_private_data.html (in core vk1.3)
#[derive(Default)]
pub struct State {
    pub instance: RwLock<Option<ash::Instance>>,
    pub device: RwLock<Option<ash::Device>>,
    pub swapchain_fn: RwLock<Option<vk::KhrSwapchainFn>>,
    pub video_queue_fn: RwLock<Option<vk::KhrVideoQueueFn>>,
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
    static STATE: Lazy<State> = Lazy::new(|| {
        let mut state = State::default();
        state.settings = Settings::new_from_env();
        state
    });
    &STATE
}

impl State {}
