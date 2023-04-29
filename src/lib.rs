use crate::vk_layer::VkLayerFunction;
use ash::vk;
use state::get_state;
use std::{mem::transmute, ptr::null_mut};
use vk_layer::{VkDevice_T, VkInstance_T, VkNegotiateLayerInterface};

mod state;
mod vk_layer;

unsafe fn ptr_chain_get_next<SRC, DST>(
    start_struct: *const SRC,
    s_type: vk::StructureType,
) -> Option<*mut DST> {
    unsafe {
        let iter = {
            // inlined (by rust-analyzer) private ptr_chain_iter from ash
            let ptr = <*const SRC>::cast::<vk::BaseOutStructure>(start_struct);
            (0..).scan(ptr, |p_ptr, _| {
                if p_ptr.is_null() {
                    return None;
                }
                let n_ptr = (**p_ptr).p_next;
                let old = *p_ptr;
                *p_ptr = n_ptr;
                Some(old)
            })
        };
        iter.filter(|s| (*(*s)).s_type == s_type)
            .map(|s| transmute(s))
            .next()
    }
}

#[no_mangle]
pub extern "system" fn record_vk_create_instance(
    p_create_info: *const vk::InstanceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_instance: *mut vk::Instance,
) -> vk::Result {
    unsafe {
        let layer_info: Option<*mut vk_layer::VkLayerInstanceCreateInfo> = ptr_chain_get_next(
            p_create_info,
            vk::StructureType::LOADER_INSTANCE_CREATE_INFO,
        );
        if let Some(layer_info) = layer_info {
            let layer_info = layer_info.as_mut().unwrap();
            if layer_info.function == VkLayerFunction::VK_LAYER_LINK_INFO {
                let state = get_state();
                *state.instance_get_fn.write().unwrap() =
                    transmute((*layer_info.u.pLayerInfo).pfnNextGetInstanceProcAddr);
                let Some(real_create_instance)  = (*layer_info.u.pLayerInfo)
                    .pfnNextGetInstanceProcAddr
                    .map(|f| f(null_mut(), transmute(b"vkCreateInstance\0"))).flatten()
                else {return vk::Result::ERROR_INITIALIZATION_FAILED};

                layer_info.u.pLayerInfo = (*layer_info.u.pLayerInfo).pNext.cast();

                let real_create_instance: vk::PFN_vkCreateInstance =
                    transmute(real_create_instance);
                // TODO: patch application info to support vk video
                let res = real_create_instance(p_create_info, p_allocator, p_instance);
                if res == vk::Result::SUCCESS {
                    *state.instance.write().unwrap() = p_instance.as_ref().copied();
                    // TODO: time to fetch instance function pointers if needed
                }

                return res;
            }
        }
    }

    vk::Result::ERROR_INITIALIZATION_FAILED
}

#[no_mangle]
pub extern "system" fn record_vk_get_instance_proc_addr(
    instance: *mut VkInstance_T,
    _fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    unsafe {
        let _instance: vk::Instance = transmute(instance);
    }
    None
}
#[no_mangle]
pub extern "system" fn record_vk_get_device_proc_addr(
    device: *mut VkDevice_T,
    _fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    unsafe {
        let _device: vk::Device = transmute(device);
    }
    None
}
#[no_mangle]
pub extern "system" fn record_vk_create_device() {
    print!("record_vk_create_device");
}
#[no_mangle]
pub extern "system" fn record_vk_negotiate_loader_layer_interface_version(
    interface: *mut VkNegotiateLayerInterface,
) -> vk::Result {
    unsafe {
        if let Some(interface) = interface.as_mut() {
            if interface.loaderLayerInterfaceVersion >= 2 {
                interface.pfnGetDeviceProcAddr = Some(record_vk_get_device_proc_addr);
                interface.pfnGetInstanceProcAddr = Some(record_vk_get_instance_proc_addr);
                interface.pfnGetPhysicalDeviceProcAddr = None;
            }
        }
    }
    vk::Result::SUCCESS
}
