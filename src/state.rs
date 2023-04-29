use std::sync::RwLock;

use ash::vk;
use once_cell::sync::Lazy;

#[derive(Default)]
pub struct State {
    pub instance: RwLock<Option<vk::Instance>>,
    pub device: RwLock<Option<vk::Device>>,
    pub instance_get_fn: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>>,
    pub device_get_fn: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>>,
}

pub fn get_state() -> &'static State {
    static STATE: Lazy<State> = Lazy::new(State::default);
    &STATE
}
