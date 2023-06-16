use std::{ffi::CString, mem::transmute};

use crate::state::Extensions;
use ash::vk;
use log::warn;

pub fn name_object<Object>(
    device: &ash::Device,
    extensions: &Extensions,
    object: Object,
    name: &str,
) where
    Object: vk::Handle + Copy,
{
    let name = CString::new(name).unwrap();
    let handle = object.as_raw();
    let info = vk::DebugUtilsObjectNameInfoEXT::default()
        .object_handle(object)
        .object_name(&name);
    unsafe {
        if let Err(err) =
            (extensions.debug_utils_fn().set_debug_utils_object_name_ext)(device.handle(), &info)
                .result_with_success(())
        {
            warn!(
                "Failed to name object {handle} of type {:?} to {:?}: {err}",
                Object::TYPE,
                name
            );
        }
    }
}

// From ash examples
pub fn find_memorytype_index(
    memory_req: &vk::MemoryRequirements,
    memory_prop: &vk::PhysicalDeviceMemoryProperties,
    flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
    memory_prop.memory_types[..memory_prop.memory_type_count as _]
        .iter()
        .enumerate()
        .find(|(index, memory_type)| {
            (1 << index) & memory_req.memory_type_bits != 0
                && memory_type.property_flags & flags == flags
        })
        .map(|(index, _memory_type)| index as _)
}

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

pub(crate) fn ptr_chain_get_next_ref<SRC, DST>(start_struct: &SRC) -> Option<&DST> {
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
        iter.map(|s| transmute(s)).next()
    }
}
