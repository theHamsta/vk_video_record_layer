use crate::vk_beta::{
    VK_KHR_VIDEO_DECODE_QUEUE_EXTENSION_NAME,
    VK_KHR_VIDEO_ENCODE_QUEUE_EXTENSION_NAME,
    //VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    VK_KHR_VIDEO_QUEUE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
    //VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
};
use crate::vk_layer::VkLayerFunction;
use ash::vk;
use log::{debug, error, info};
use state::get_state;
use std::{
    collections::HashSet,
    ffi::{c_void, CStr},
    mem::transmute,
    ptr::null_mut,
};
use vk_layer::{VkDevice_T, VkInstance_T, VkNegotiateLayerInterface};

mod settings;
mod state;
mod video_session;
mod vk_beta;
mod vk_layer;

unsafe fn ptr_chain_get_next<SRC, DST>(
    start_struct: &SRC,
    predicate: impl Fn(&*const vk::BaseOutStructure) -> bool,
) -> Option<*mut DST> {
    unsafe {
        let iter = {
            // inlined (by rust-analyzer): private ptr_chain_iter from ash
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
    debug!("record_vk_create_instance");
    unsafe {
        let layer_info: Option<*mut vk_layer::VkLayerInstanceCreateInfo> =
            ptr_chain_get_next(p_create_info.as_ref().unwrap(), |&b| -> bool {
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
                let get_instance_proc_addr = (*layer_info.u.pLayerInfo).pfnNextGetInstanceProcAddr;
                let Some(real_create_instance)  = get_instance_proc_addr
                    .map(|f| f(null_mut(), transmute(b"vkCreateInstance\0"))).flatten()
                else { return vk::Result::ERROR_INITIALIZATION_FAILED };

                layer_info.u.pLayerInfo = (*layer_info.u.pLayerInfo).pNext.cast();

                let real_create_instance: vk::PFN_vkCreateInstance =
                    transmute(real_create_instance);
                // TODO: patch application info to support vk video
                let res = real_create_instance(p_create_info, p_allocator, p_instance);
                if res == vk::Result::SUCCESS {
                    *state.instance.write().unwrap() = Some(ash::Instance::load(
                        &vk::StaticFn {
                            get_instance_proc_addr: transmute(get_instance_proc_addr),
                        },
                        p_instance.as_ref().copied().unwrap(),
                    ));
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
    debug!("record_vk_get_instance_proc_addr");
    unsafe {
        if instance.is_null() {
            return None;
        }
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

#[no_mangle]
pub extern "system" fn record_vk_get_device_proc_addr(
    device: *mut VkDevice_T,
    fn_name: *const i8,
) -> vk::PFN_vkVoidFunction {
    debug!("record_vk_get_device_proc_addr");
    unsafe {
        if device.is_null() {
            return None;
        }
        let device: vk::Device = vk::Handle::from_raw(device as u64);
        let str_fn_name = CStr::from_ptr(fn_name).to_str().unwrap();
        debug!("{device:?} {str_fn_name:?}");
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
    debug!("record_vk_create_device");

    unsafe {
        let layer_info: Option<*mut vk_layer::VkLayerDeviceCreateInfo> =
            ptr_chain_get_next(p_create_info.as_ref().unwrap(), |&b| -> bool {
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
                let get_device_proc_addr =
                    transmute((*layer_info.u.pLayerInfo).pfnNextGetDeviceProcAddr);
                *state.device_get_fn.write().unwrap() = get_device_proc_addr;
                let lock = state.instance.read().unwrap();
                let instance = lock.as_ref().unwrap();
                let get_instance_proc_addr = (*layer_info.u.pLayerInfo).pfnNextGetInstanceProcAddr;

                let Some(real_create_device)  = get_instance_proc_addr
                    .map(|f| f(transmute(lock.as_ref().unwrap().handle()), b"vkCreateDevice\0".as_ptr()as *const i8)).flatten()
                else { return vk::Result::ERROR_INITIALIZATION_FAILED };

                layer_info.u.pLayerInfo = (*layer_info.u.pLayerInfo).pNext.cast();

                let real_create_device: vk::PFN_vkCreateDevice = transmute(real_create_device);

                let create_info = *p_create_info;
                const REQUIRED_EXTENSIONS: [&'static CStr; 5] = unsafe {
                    [
                        CStr::from_bytes_with_nul_unchecked(VK_KHR_VIDEO_QUEUE_EXTENSION_NAME),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_KHR_VIDEO_DECODE_QUEUE_EXTENSION_NAME,
                        ),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_KHR_VIDEO_ENCODE_QUEUE_EXTENSION_NAME,
                        ),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
                        ),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
                        ),
                    ]
                };

                let mut extensions: HashSet<&CStr> = (0isize
                    ..create_info.enabled_extension_count as isize)
                    .map(|i| CStr::from_ptr((*create_info.pp_enabled_extension_names).offset(i)))
                    .collect();
                info!("Enabled extensions: {:?}", extensions);
                // TODO check whether they are supported
                for e in REQUIRED_EXTENSIONS.iter() {
                    extensions.insert(e);
                }
                info!("Enabled extensions after layer: {:?}", extensions);
                let extensions: Vec<_> =
                    extensions.iter().map(|s| s.as_ptr() as *const i8).collect();

                create_info.enabled_extension_names(&extensions);

                // will we get arrested when using this without vk1.1,vk1.2 instance fns, because
                // we were too lazy to patch instance create info?
                let mut queue_props = Vec::new();
                queue_props.resize(
                    instance.get_physical_device_queue_family_properties2_len(physical_device),
                    vk::QueueFamilyProperties2::default(),
                );
                instance.get_physical_device_queue_family_properties2(
                    physical_device,
                    &mut queue_props,
                );

                let mut device_queues: Vec<vk::DeviceQueueCreateInfo> = (0isize
                    ..create_info.queue_create_info_count as isize)
                    .map(|i| *create_info.p_queue_create_infos.offset(i))
                    .collect();

                let Some(compute_idx) = queue_props.iter().position(|prop| {
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::COMPUTE)
                }) else {
                    error!("Device doesn't support compute");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                };
                let Some(encode_idx) = queue_props.iter().position(|prop| {
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::VIDEO_ENCODE_KHR)
                }) else {
                    error!("Device doesn't support encode");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                };
                let Some(decode_idx) = queue_props.iter().position(|prop| {
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::VIDEO_DECODE_KHR)
                }) else {
                    error!("Device doesn't support decode");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                };

                let compute_queue = device_queues
                    .iter()
                    .find(|q| q.queue_family_index as usize == compute_idx);
                if !compute_queue.is_some() {
                    info!(
                        "App didn't request a queue with compute bit! So we're doing it right now"
                    );
                    device_queues.push(
                        vk::DeviceQueueCreateInfo::default()
                            .queue_family_index(compute_idx as u32)
                            .queue_priorities(&[1.0]),
                    );
                }
                let encode_queue = device_queues
                    .iter()
                    .find(|q| q.queue_family_index as usize == compute_idx);
                if !encode_queue.is_some() {
                    error!("App already requested a queue with encode bit!");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                }

                let decode_queue = device_queues
                    .iter()
                    .find(|q| q.queue_family_index as usize == compute_idx);
                if !decode_queue.is_some() {
                    error!("App already requested a queue with decode bit!");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                }

                device_queues.push(
                    vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(encode_idx as u32)
                        .queue_priorities(&[1.0]),
                );
                device_queues.push(
                    vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(decode_idx as u32)
                        .queue_priorities(&[1.0]),
                );
                create_info.queue_create_infos(&device_queues);
                info!("{create_info:?}");

                // TODO: patch application info to support vk video
                let res = real_create_device(physical_device, &create_info, p_allocator, p_device);
                if res == vk::Result::SUCCESS {
                    let device = transmute(*p_device);

                    let device = ash::Device::load(
                        &vk::InstanceFnV1_0 {
                            get_device_proc_addr: transmute(get_device_proc_addr),
                            destroy_instance: transmute(1u64), // Rust function pointer must be
                            // non-null :shrug:
                            enumerate_physical_devices: transmute(1u64),
                            get_physical_device_features: transmute(1u64),
                            get_physical_device_format_properties: transmute(1u64),
                            get_physical_device_image_format_properties: transmute(1u64),
                            get_physical_device_properties: transmute(1u64),
                            get_physical_device_queue_family_properties: transmute(1u64),
                            get_physical_device_memory_properties: transmute(1u64),
                            create_device: transmute(1u64),
                            enumerate_device_extension_properties: transmute(1u64),
                            enumerate_device_layer_properties: transmute(1u64),
                            get_physical_device_sparse_image_format_properties: transmute(1u64),
                        },
                        device,
                    );
                    *state.compute_queue.write().unwrap() =
                        Some(device.get_device_queue(compute_idx as u32, 0));
                    *state.encode_queue.write().unwrap() =
                        Some(device.get_device_queue(encode_idx as u32, 0));
                    *state.decode_queue.write().unwrap() =
                        Some(device.get_device_queue(decode_idx as u32, 0));
                    *state.device.write().unwrap() = Some(device);

                    return vk::Result::SUCCESS;
                }
            }
        }
    }

    vk::Result::ERROR_INITIALIZATION_FAILED
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
