use crate::vk_layer::VkLayerFunction;
use ash::vk;
use state::get_state;
use std::{
    ffi::{c_void, CStr},
    mem::transmute,
    ptr::null_mut,
};
use vk_layer::{VkDevice_T, VkInstance_T, VkNegotiateLayerInterface};

mod state;
mod vk_layer;

unsafe fn ptr_chain_get_next<SRC, DST>(
    start_struct: *const SRC,
    predicate: fn(&*const vk::BaseOutStructure) -> bool,
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
        iter.filter(predicate).map(|s| transmute(s)).next()
    }
}

#[no_mangle]
pub extern "system" fn record_vk_create_instance(
    p_create_info: *const vk::InstanceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_instance: *mut vk::Instance,
) -> vk::Result {
    eprintln!("record_vk_create_instance");
    unsafe {
        let layer_info: Option<*mut vk_layer::VkLayerInstanceCreateInfo> =
            ptr_chain_get_next(p_create_info, |&b| -> bool {
                (*b).s_type == vk::StructureType::LOADER_INSTANCE_CREATE_INFO
                    && (*b.cast::<vk_layer::VkLayerInstanceCreateInfo>()).function
                        == VkLayerFunction::VK_LAYER_LINK_INFO
            });
        if let Some(layer_info) = layer_info {
            debug_assert!(
                (*layer_info).sType == transmute(vk::StructureType::LOADER_INSTANCE_CREATE_INFO)
            );

            let layer_info = layer_info.as_mut().unwrap();
            if layer_info.function == VkLayerFunction::VK_LAYER_LINK_INFO {
                let state = get_state();
                *state.instance_get_fn.write().unwrap() =
                    transmute((*layer_info.u.pLayerInfo).pfnNextGetInstanceProcAddr);
                let Some(real_create_instance)  = (*layer_info.u.pLayerInfo)
                    .pfnNextGetInstanceProcAddr
                    .map(|f| f(null_mut(), transmute(b"vkCreateInstance\0"))).flatten()
                else { return vk::Result::ERROR_INITIALIZATION_FAILED };

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
    fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    eprintln!("record_vk_get_instance_proc_addr");
    unsafe {
        let instance: vk::Instance = transmute(instance);
        let str_fn_name = CStr::from_ptr(fn_name).to_str().unwrap();
        eprintln!("{instance:?} {str_fn_name:?}");
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

#[no_mangle]
pub extern "system" fn record_vk_get_device_proc_addr(
    device: *mut VkDevice_T,
    fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    eprintln!("record_vk_get_device_proc_addr");
    unsafe {
        let device: vk::Device = transmute(device);
        let str_fn_name = CStr::from_ptr(fn_name).to_str().unwrap();
        eprintln!("{device:?} {str_fn_name:?}");
        match str_fn_name {
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
pub extern "system" fn record_vk_create_device(
    physical_device: vk::PhysicalDevice,
    p_create_info: *const vk::DeviceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_device: *mut vk::Device,
) -> vk::Result {
    eprintln!("record_vk_create_device");

    unsafe {
        let layer_info: Option<*mut vk_layer::VkLayerDeviceCreateInfo> =
            ptr_chain_get_next(p_create_info, |&b| -> bool {
                (*b).s_type == vk::StructureType::LOADER_DEVICE_CREATE_INFO
                    && (*b.cast::<vk_layer::VkLayerDeviceCreateInfo>()).function
                        == VkLayerFunction::VK_LAYER_LINK_INFO
            });
        if let Some(layer_info) = layer_info {
            debug_assert!(
                (*layer_info).sType == transmute(vk::StructureType::LOADER_DEVICE_CREATE_INFO)
            );

            let layer_info = layer_info.as_mut().unwrap();
            if layer_info.function == VkLayerFunction::VK_LAYER_LINK_INFO {
                let state = get_state();
                *state.device_get_fn.write().unwrap() =
                    transmute((*layer_info.u.pLayerInfo).pfnNextGetDeviceProcAddr);
                let Some(real_create_device)  = (*layer_info.u.pLayerInfo)
                    .pfnNextGetInstanceProcAddr
                    .map(|f| f(transmute(get_state().instance.read().unwrap().unwrap()), transmute(b"vkCreateDevice\0"))).flatten()
                else { return vk::Result::ERROR_INITIALIZATION_FAILED };

                layer_info.u.pLayerInfo = (*layer_info.u.pLayerInfo).pNext.cast();

                let real_create_device: vk::PFN_vkCreateDevice = transmute(real_create_device);
                // TODO: patch application info to support vk video
                let res = real_create_device(physical_device, p_create_info, p_allocator, p_device);
                if res == vk::Result::SUCCESS {
                    *state.device.write().unwrap() = p_device.as_ref().copied();
                    // TODO: time to fetch instance function pointers if needed
                }

                return res;
            }
        }
    }

    vk::Result::ERROR_INITIALIZATION_FAILED
}

#[no_mangle]
pub extern "system" fn record_vk_negotiate_loader_layer_interface_version(
    interface: *mut VkNegotiateLayerInterface,
) -> vk::Result {
    eprintln!("record_vk_negotiate_loader_layer_interface_version");
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
