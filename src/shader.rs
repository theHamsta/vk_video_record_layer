use anyhow::{anyhow, bail};
use ash::{util::read_spv, vk};
use itertools::Itertools;
use log::debug;
use spirv_reflect::types::ReflectDescriptorBinding;
use std::collections::HashMap;
use std::ffi::CString;
use std::intrinsics::transmute;
use std::io::Cursor;

pub struct Shader {
    module: vk::ShaderModule,
    info: spirv_reflect::ShaderModule,
}

pub struct ShaderPipeline {
    shaders: Vec<Shader>,
}

impl ShaderPipeline {
    pub fn destroy(&mut self, device: &ash::Device, allocator: Option<&vk::AllocationCallbacks>) {
        for s in self.shaders.drain(..) {
            unsafe { device.destroy_shader_module(s.module, allocator) };
        }
    }

    pub fn new(device: &ash::Device, shader_bytes: &[&[u8]]) -> anyhow::Result<Self> {
        let mut shaders = Vec::new();
        for &bytes in shader_bytes {
            let info = spirv_reflect::ShaderModule::load_u8_data(bytes)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            debug!(
                "Loaded shader {:?} ({:?}) in: {:?}, out: {:?} _push_constant_blocks {:?}",
                info.get_source_file(),
                info.get_shader_stage(),
                info.enumerate_input_variables(None),
                info.enumerate_output_variables(None),
                info.enumerate_push_constant_blocks(None)
            );

            shaders.push(Shader {
                module: unsafe {
                    device.create_shader_module(
                        &vk::ShaderModuleCreateInfo::default()
                            .code(&read_spv(&mut Cursor::new(bytes))?),
                        None,
                    )?
                },
                info,
            });
        }
        Ok(Self { shaders })
    }

    pub fn make_compute_pipeline(
        &self,
        device: &ash::Device,
        entry_point: &str,
        push_constant_ranges: &[vk::PushConstantRange], // TODO: do this via reflection
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> anyhow::Result<(
        vk::Pipeline,
        vk::PipelineLayout,
        Vec<vk::DescriptorSetLayout>,
    )> {
        let mut bindings: HashMap<u32, Vec<vk::DescriptorSetLayoutBinding>> = Default::default();
        for &ReflectDescriptorBinding {
            set,
            binding,
            descriptor_type,
            count,
            ..
        } in self.shaders[0]
            .info
            .enumerate_descriptor_bindings(Some(entry_point))
            .map_err(|str| anyhow!("{str}"))?
            .iter()
        {
            bindings.entry(set).or_default().push(
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(unsafe {
                        transmute(transmute::<_, u8>(descriptor_type) as u32)
                    })
                    .descriptor_count(count)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
            )
        }
        let mut layouts = bindings
            .iter()
            .map_while(|(set, bindings)| {
                let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(bindings);
                unsafe {
                    Some((
                        set,
                        device.create_descriptor_set_layout(&info, allocator).ok()?,
                    ))
                }
            })
            .sorted_unstable_by_key(|&(set, _bindings)| set)
            .map(|(_set, bindings)| bindings)
            .collect_vec();

        // When we could not create layouts all layouts
        if layouts.len() != bindings.len() {
            for layout in layouts.drain(..) {
                unsafe { device.destroy_descriptor_set_layout(layout, allocator) };
            }
            bail!("Failed to create all descriptor set layouts");
        }

        let layout_create_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&layouts)
            .push_constant_ranges(push_constant_ranges);

        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&layout_create_info, allocator) }
                .map_err(|e| anyhow!("Failed to create pipeline layout: {e}"))?; //TODO: unwrap

        let shader = &self.shaders[0];
        let entry_point =
            CString::new(entry_point).expect("Could not convert entry point to CStr!");
        let shader_stage_create_info = vk::PipelineShaderStageCreateInfo::default()
            .name(&entry_point)
            .module(shader.module)
            .stage(unsafe { transmute(shader.info.get_shader_stage()) });

        let pipeline = unsafe {
            device.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(shader_stage_create_info)
                    .layout(pipeline_layout)],
                allocator,
            )
        }
        .map_err(|(pipelines, r)| {
            for p in pipelines {
                unsafe { device.destroy_pipeline(p, allocator) };
            }
            r
        })?[0];

        Ok((pipeline, pipeline_layout, layouts))
    }
}
