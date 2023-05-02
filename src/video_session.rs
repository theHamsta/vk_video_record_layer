use ash::vk;

use crate::state::get_state;

struct SwapChainData {
    resolution: vk::Extent2D,
    swapchain_format: vk::Format,
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
    let result = (get_state()
        .swapchain_fn
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .create_swapchain_khr)(device, p_create_info, p_allocator, p_swapchain);
    if result == vk::Result::SUCCESS {
        let slot = get_state().private_slot.read().unwrap();
        let lock = get_state().device.read().unwrap();
        let device = lock.as_ref().unwrap();
        let create_info = p_create_info.as_ref().unwrap();
        /*let result = */
        device
            .set_private_data(
                *p_swapchain,
                *slot,
                Box::leak(Box::new(|| SwapChainData {
                    resolution: create_info.image_extent,
                    swapchain_format: create_info.image_format,
                    video_format: vk::Format::G8_B8R8_2PLANE_420_UNORM,
                    encode_session: vk::VideoSessionKHR::null(),
                    decode_session: vk::VideoSessionKHR::null(),
                })) as *const _ as u64,
            )
            .unwrap(); // TODO
    }
    result
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
        .queue_present_khr)(queue, p_present_info)
}
