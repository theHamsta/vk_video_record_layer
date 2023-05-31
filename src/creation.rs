use ash::vk;
use core::ptr::null_mut;

use crate::state::get_state;
use crate::vk_beta::{
    VK_KHR_VIDEO_DECODE_QUEUE_EXTENSION_NAME,
    VK_KHR_VIDEO_ENCODE_QUEUE_EXTENSION_NAME,
    //VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    VK_KHR_VIDEO_QUEUE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_DECODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
    //VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
    VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
};
use crate::vk_layer;
use crate::vk_layer::VkLayerFunction;
use log::{debug, error, info};
use std::collections::HashSet;
use std::{ffi::CStr, mem::transmute};

pub(crate) unsafe fn ptr_chain_get_next<SRC, DST>(
    start_struct: *const SRC,
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
    p_create_info: *mut vk::InstanceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_instance: *mut vk::Instance,
) -> vk::Result {
    debug!("record_vk_create_instance");
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
                let get_instance_proc_addr = (*layer_info.u.pLayerInfo).pfnNextGetInstanceProcAddr;
                let Some(real_create_instance)  = get_instance_proc_addr
                    .map(|f| f(null_mut(), transmute(b"vkCreateInstance\0"))).flatten()
                else { return vk::Result::ERROR_INITIALIZATION_FAILED };

                layer_info.u.pLayerInfo = (*layer_info.u.pLayerInfo).pNext.cast();
                let create_info = p_create_info.as_mut().unwrap().clone();
                let app_info = (*(*p_create_info).p_application_info)
                    .api_version(vk::make_api_version(0, 1, 3, 249));
                let create_info = create_info.application_info(&app_info);

                let real_create_instance: vk::PFN_vkCreateInstance =
                    transmute(real_create_instance);
                // TODO: patch application info to support vk video
                let res = real_create_instance(&create_info, p_allocator, p_instance);
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
pub extern "system" fn record_vk_create_device(
    physical_device: vk::PhysicalDevice,
    p_create_info: *const vk::DeviceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_device: *mut vk::Device,
) -> vk::Result {
    debug!("record_vk_create_device");

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

                const REQUIRED_EXTENSIONS: [&'static CStr; 7] = unsafe {
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
                            VK_STD_VULKAN_VIDEO_CODEC_H265_DECODE_EXTENSION_NAME,
                        ),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_STD_VULKAN_VIDEO_CODEC_H264_ENCODE_EXTENSION_NAME,
                        ),
                        CStr::from_bytes_with_nul_unchecked(
                            VK_STD_VULKAN_VIDEO_CODEC_H265_ENCODE_EXTENSION_NAME,
                        ),
                    ]
                };

                let mut create_info = p_create_info.cast_mut().as_mut().unwrap().clone();
                let mut extensions: HashSet<&CStr> = (0isize
                    ..(*p_create_info).enabled_extension_count as isize)
                    .map(|i| {
                        CStr::from_ptr((*(*p_create_info).pp_enabled_extension_names).offset(i))
                    })
                    .collect();
                info!("Enabled extensions: {:?}", extensions);
                // TODO check whether they are supported
                for e in REQUIRED_EXTENSIONS.iter() {
                    extensions.insert(e);
                }
                info!("Enabled extensions after layer: {:?}", extensions);
                let extensions: Vec<_> =
                    extensions.iter().map(|s| s.as_ptr() as *const i8).collect();

                //*p_create_info = (*p_create_info).enabled_extension_names(&extensions);
                create_info.enabled_extension_count = extensions.len() as u32;
                create_info.pp_enabled_extension_names = extensions.as_ptr();

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
                let Some(graphics_idx) = queue_props.iter().position(|prop| {
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::GRAPHICS)
                }) else {
                    error!("Device doesn't support graphics");
                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                };
                let Some(encode_idx) = queue_props.iter().position(|prop| {
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::VIDEO_ENCODE_KHR) &&
                    prop.queue_family_properties
                        .queue_flags
                        .contains(vk::QueueFlags::TRANSFER)
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
                //create_info.queue_create_infos(&device_queues);
                create_info.queue_create_info_count = device_queues.len() as u32;
                create_info.p_queue_create_infos = device_queues.as_ptr();
                //create_info.queue_create_infos(&device_queues);
                info!("{create_info:?}");

                let mut features11 =
                    vk::PhysicalDeviceVulkan11Features::default().sampler_ycbcr_conversion(true);
                let mut features12 = vk::PhysicalDeviceVulkan12Features::default()
                    .timeline_semaphore(true)
                    .buffer_device_address(true);
                //.vulkan_memory_model(true);
                let mut features13 = vk::PhysicalDeviceVulkan13Features::default()
                    .private_data(true)
                    .synchronization2(true);

                if !ptr_chain_get_next::<_, vk::BaseOutStructure>(&create_info, |c| {
                    (*(*c)).s_type == features11.s_type
                })
                .is_some()
                {
                    create_info = create_info.push_next(&mut features11);
                }
                if !ptr_chain_get_next::<_, vk::BaseOutStructure>(&create_info, |c| {
                    (*(*c)).s_type == features12.s_type
                })
                .is_some()
                {
                    create_info = create_info.push_next(&mut features12);
                }
                if !ptr_chain_get_next::<_, vk::BaseOutStructure>(&create_info, |c| {
                    (*(*c)).s_type == features13.s_type
                })
                .is_some()
                {
                    create_info = create_info.push_next(&mut features13);
                }
                debug_assert!(!create_info.p_next.is_null());
                debug_assert!(!(*p_create_info).p_next.is_null());

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
                    *state.physical_device.write().unwrap() = Some(physical_device);

                    // Load extensions
                    let swapchain_fn = vk::KhrSwapchainFn::load(|name| {
                        transmute((get_device_proc_addr.unwrap())(
                            device.handle(),
                            name.as_ptr() as *const _,
                        ))
                    });
                    *state.swapchain_fn.write().unwrap() = Some(swapchain_fn);

                    let video_queue_fn = vk::KhrVideoQueueFn::load(|name| {
                        transmute((get_device_proc_addr.unwrap())(
                            device.handle(),
                            name.as_ptr() as *const _,
                        ))
                    });
                    *state.video_queue_fn.write().unwrap() = Some(video_queue_fn);

                    let video_encode_queue_fn = vk::KhrVideoEncodeQueueFn::load(|name| {
                        transmute((get_device_proc_addr.unwrap())(
                            device.handle(),
                            name.as_ptr() as *const _,
                        ))
                    });
                    *state.video_encode_queue_fn.write().unwrap() = Some(video_encode_queue_fn);

                    let Ok(slot) = device.create_private_data_slot(
                        &vk::PrivateDataSlotCreateInfo::default(),
                        p_allocator.as_ref(),
                    ) else {
                        error!("Failed to allocate private data");
                        return vk::Result::ERROR_INITIALIZATION_FAILED;
                    };
                    *state.private_slot.write().unwrap() = slot;

                    *state.graphics_queue_family_idx.write().unwrap() = graphics_idx as u32;
                    *state.compute_queue_family_idx.write().unwrap() = compute_idx as u32;
                    *state.encode_queue_family_idx.write().unwrap() = encode_idx as u32;
                    *state.decode_queue_family_idx.write().unwrap() = decode_idx as u32;
                    *state.device.write().unwrap() = Some(device);

                    return vk::Result::SUCCESS;
                }
            }
        }
    }

    vk::Result::ERROR_INITIALIZATION_FAILED
}
