use anyhow::{anyhow, bail};
use ash::{util::read_spv, vk};
use itertools::Itertools;
use log::debug;
use std::collections::HashMap;
use std::ffi::CString;
use std::intrinsics::transmute;
use std::io::Cursor;

pub struct Shader {
    module: vk::ShaderModule,
    info: rspirv_reflect::Reflection,
}

pub struct ShaderPipeline {
    shaders: Vec<Shader>,
}

#[derive(Default)]
pub struct ComputePipelineDescriptor {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    descriptor_set_layouts: Vec<vk::DescriptorSetLayout>,
    #[allow(dead_code)]
    push_constant_ranges: Vec<vk::PushConstantRange>,
}

impl ComputePipelineDescriptor {
    pub fn pipeline(&self) -> vk::Pipeline {
        self.pipeline
    }

    pub fn layout(&self) -> vk::PipelineLayout {
        self.layout
    }

    pub fn descriptor_set_layouts(&self) -> &[vk::DescriptorSetLayout] {
        self.descriptor_set_layouts.as_ref()
    }

    pub(crate) fn destroy(
        &mut self,
        device: &ash::Device,
        allocator: Option<&vk::AllocationCallbacks>,
    ) {
        unsafe {
            device.destroy_pipeline(self.pipeline, allocator);
            for layout in self.descriptor_set_layouts.drain(..) {
                device.destroy_descriptor_set_layout(layout, allocator);
            }
        }
    }
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
            let info = rspirv_reflect::Reflection::new_from_spirv(bytes)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            debug!("Loaded shader",);

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
        allocator: Option<&vk::AllocationCallbacks>,
    ) -> anyhow::Result<ComputePipelineDescriptor> {
        let mut bindings: HashMap<u32, Vec<vk::DescriptorSetLayoutBinding>> = Default::default();
        for (&set, info) in self.shaders[0]
            .info
            .get_descriptor_sets()
            .map_err(|str| anyhow!("{str}"))?
            .iter()
        {
            for (&binding, info) in info.iter() {
                bindings.entry(set).or_default().push(
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(binding)
                        .descriptor_type(unsafe { transmute(transmute::<_, u32>(info.ty)) })
                        .descriptor_count(match info.binding_count {
                            rspirv_reflect::BindingCount::One => 1,
                            rspirv_reflect::BindingCount::StaticSized(size) => size as u32,
                            rspirv_reflect::BindingCount::Unbounded => todo!(),
                        })
                        .stage_flags(vk::ShaderStageFlags::COMPUTE),
                )
            }
        }
        let mut descriptor_set_layouts = bindings
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

        let push_constant_ranges: Result<Vec<vk::PushConstantRange>, _> =
            self.shaders[0].info.get_push_constant_range().map(|info| {
                info.map(|info| {
                    vk::PushConstantRange::default()
                        .size(info.size)
                        .offset(info.offset)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE)
                })
                .iter()
                .copied()
                .collect()
            });

        // When we could not create layouts all layouts
        if descriptor_set_layouts.len() != bindings.len() || push_constant_ranges.is_err() {
            for layout in descriptor_set_layouts.drain(..) {
                unsafe { device.destroy_descriptor_set_layout(layout, allocator) };
            }
            bail!("Failed to create all descriptor set layouts");
        }
        let push_constant_ranges = push_constant_ranges.unwrap();

        let layout_create_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&descriptor_set_layouts)
            .push_constant_ranges(&push_constant_ranges);

        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&layout_create_info, allocator) }
                .map_err(|e| anyhow!("Failed to create pipeline layout: {e}"))?; //TODO: unwrap

        let shader = &self.shaders[0];
        let entry_point =
            CString::new(entry_point).expect("Could not convert entry point to CStr!");
        let shader_stage_create_info = vk::PipelineShaderStageCreateInfo::default()
            .name(&entry_point)
            .module(shader.module)
            .stage(vk::ShaderStageFlags::COMPUTE);

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

        Ok(ComputePipelineDescriptor {
            pipeline,
            layout: pipeline_layout,
            descriptor_set_layouts,
            push_constant_ranges,
        })
    }
}
