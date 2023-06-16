mod buffer_queue;
mod cmd_buffer_queue;
mod creation;
mod dpb;
mod profile;
mod session_parameters;
mod settings;
mod shader;
mod state;
mod video_session;
mod vk_beta;
mod vk_layer;
mod vulkan_utils;
mod bitstream;

use crate::creation::{record_vk_create_device, record_vk_create_instance};
use crate::video_session::{
    record_vk_create_swapchain, record_vk_destroy_swapchain, record_vk_queue_present,
};
use ash::vk;
use log::{debug, trace};
use state::get_state;
use std::{
    ffi::{c_void, CStr},
    mem::transmute,
};
use vk_layer::{VkDevice_T, VkInstance_T, VkNegotiateLayerInterface};

#[no_mangle]
pub extern "system" fn record_vk_get_instance_proc_addr(
    instance: *mut VkInstance_T,
    fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    trace!("record_vk_get_instance_proc_addr");
    unsafe {
        let instance: vk::Instance = vk::Handle::from_raw(instance as u64);
        let str_fn_name = CStr::from_ptr(fn_name).to_str().unwrap();
        debug!("{instance:?} {str_fn_name:?}");
        match str_fn_name {
            "vkCreateDevice" => Some(transmute(record_vk_create_device as *mut c_void)),
            "vkCreateInstance" => Some(transmute(record_vk_create_instance as *mut c_void)),
            _ => {
                let state = get_state();
                let get_fn = state.instance_get_fn.read().unwrap();
                if let Some(get_fn) = get_fn.as_ref() {
                    (get_fn)(instance, fn_name)
                } else {
                    None
                }
            }
        }
    }
}

//fn record_vk_aquire_next_image() -> vk::Result {
//vk::Result::SUCCESS
//}

#[no_mangle]
pub extern "system" fn record_vk_get_device_proc_addr(
    device: *mut VkDevice_T,
    fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    trace!("record_vk_get_device_proc_addr");
    unsafe {
        if device.is_null() {
            return None;
        }
        let device: vk::Device = vk::Handle::from_raw(device as u64);
        let str_fn_name = CStr::from_ptr(fn_name).to_str().unwrap();
        trace!("{device:?} {str_fn_name:?}");
        match str_fn_name {
            "vkCreateSwapchainKHR" => Some(transmute(record_vk_create_swapchain as *mut c_void)),
            //"vkAcquireNextImageKHR" => Some(transmute(record_vk_aquire_next_image as *mut c_void)),
            "vkDestroySwapchainKHR" => Some(transmute(record_vk_destroy_swapchain as *mut c_void)),
            "vkDestroyDevice" => Some(transmute(record_vk_destroy_device as *mut c_void)),
            "vkQueuePresentKHR" => Some(transmute(record_vk_queue_present as *mut c_void)),
            _ => {
                let state = get_state();
                let get_fn = state.device_get_fn.read().unwrap();
                if let Some(get_fn) = get_fn.as_ref() {
                    (get_fn)(device, fn_name)
                } else {
                    None
                }
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn record_vk_destroy_device(
    device: vk::Device,
    p_allocator: *const vk::AllocationCallbacks,
) {
    debug!("record_vk_destroy_device");
    if device != vk::Device::null() {
        let state = get_state();
        let lock = state.device.write().unwrap();
        let device = lock.as_ref().unwrap();
        let slot = *state.private_slot.write().unwrap();
        unsafe {
            let allocator = p_allocator.as_ref();
            device.destroy_private_data_slot(slot, allocator);
            device.destroy_device(allocator);
        }
    }
}

#[no_mangle]
pub extern "system" fn record_vk_negotiate_loader_layer_interface_version(
    interface: *mut VkNegotiateLayerInterface,
) -> vk::Result {
    let _ = pretty_env_logger::try_init();
    debug!("record_vk_negotiate_loader_layer_interface_version");
    unsafe {
        if let Some(interface) = interface.as_mut() {
            if interface.loaderLayerInterfaceVersion >= 2 {
                interface.pfnGetDeviceProcAddr = Some(record_vk_get_device_proc_addr);
                interface.pfnGetInstanceProcAddr = Some(record_vk_get_instance_proc_addr);
                //interface.pfnGetPhysicalDeviceProcAddr = None;
            }
        }
    }
    vk::Result::SUCCESS
}
