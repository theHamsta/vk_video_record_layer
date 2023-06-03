use std::ffi::CString;

use crate::state::Extensions;
use ash::vk;
use log::warn;


pub fn name_object<Object>(
    device: &ash::Device,
    extensions: &Extensions,
    object: Object,
    name: &str,
) where
    Object: vk::Handle,
{
    let name = CString::new(name).unwrap();
    let handle = object.as_raw();
    let info = vk::DebugUtilsObjectNameInfoEXT::default()
        .object_handle(handle)
        .object_type(Object::TYPE)
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

