use ash::vk;

use crate::state::get_state;

struct SwapChainData {
    resolution: vk::Extent2D,
    swapchain_format: vk::Format,
    swapchain_images: u32,
    video_format: vk::Format,
    encode_session: vk::VideoSessionKHR,
    decode_session: vk::VideoSessionKHR,
}

impl SwapChainData {}

pub unsafe fn record_vk_create_swapchain(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> vk::Result {
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .fp()
        .create_swapchain_khr)(device, p_create_info, p_allocator, p_swapchain)
}

pub unsafe extern "system" fn record_vk_destroy_swapchain(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_allocator: *const vk::AllocationCallbacks,
) {
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .fp()
        .destroy_swapchain_khr)(device, swapchain, p_allocator)
}

pub unsafe extern "system" fn record_vk_queue_present(
    queue: vk::Queue,
    p_present_info: *const vk::PresentInfoKHR,
) -> vk::Result {
    (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .fp()
        .queue_present_khr)(queue, p_present_info)
}
