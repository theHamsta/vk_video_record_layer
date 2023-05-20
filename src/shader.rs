use anyhow::{anyhow, bail};
use ash::vk::{VertexInputAttributeDescription, VertexInputBindingDescription};
use ash::{util::read_spv, vk};
use itertools::Itertools;
use log::debug;
use spirv_reflect::types::ReflectDescriptorBinding;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
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
                //alt_info,
            });
        }
        Ok(Self { shaders })
    }

    pub fn make_graphics_pipeline(
        &self,
        device: &ash::Device,
        scissors: &[vk::Rect2D],
        viewports: &[vk::Viewport],
        format: vk::Format,
        vertex_input_attribute_descriptions: &[VertexInputAttributeDescription],
        vertex_input_binding_descriptions: &[VertexInputBindingDescription],
        push_constant_ranges: &[vk::PushConstantRange], // TODO: do this via reflection
    ) -> anyhow::Result<(vk::Pipeline, vk::RenderPass, vk::PipelineLayout)> {
        let shader_entry_name = unsafe { CStr::from_bytes_with_nul_unchecked(b"main\0") };
        let shader_stage_create_infos = self
            .shaders
            .iter()
            .map(|shader| {
                vk::PipelineShaderStageCreateInfo::default()
                    .name(shader_entry_name)
                    .module(shader.module)
                    .stage(unsafe { transmute(shader.info.get_shader_stage()) })
            })
            .collect::<Vec<_>>();

        let vertex_input_state_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_attribute_descriptions(vertex_input_attribute_descriptions)
            .vertex_binding_descriptions(vertex_input_binding_descriptions);
        let vertex_input_assembly_state_info = vk::PipelineInputAssemblyStateCreateInfo {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            ..Default::default()
        };
        let rasterization_info = vk::PipelineRasterizationStateCreateInfo {
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            line_width: 1.0,
            polygon_mode: vk::PolygonMode::FILL,

            cull_mode: vk::CullModeFlags::BACK,
            ..Default::default()
        };
        let multisample_state_info = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };
        let noop_stencil_state = vk::StencilOpState {
            fail_op: vk::StencilOp::KEEP,
            pass_op: vk::StencilOp::KEEP,
            depth_fail_op: vk::StencilOp::KEEP,
            compare_op: vk::CompareOp::ALWAYS,
            ..Default::default()
        };
        let depth_state_info = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: 1,
            depth_write_enable: 1,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            front: noop_stencil_state,
            back: noop_stencil_state,
            max_depth_bounds: 1.0,
            ..Default::default()
        };
        let color_blend_attachment_states = [vk::PipelineColorBlendAttachmentState {
            blend_enable: 0,
            src_color_blend_factor: vk::BlendFactor::SRC_COLOR,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_DST_COLOR,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::ZERO,
            dst_alpha_blend_factor: vk::BlendFactor::ZERO,
            alpha_blend_op: vk::BlendOp::ADD,
            color_write_mask: vk::ColorComponentFlags::RGBA,
        }];
        let color_blend_state = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op(vk::LogicOp::CLEAR)
            .attachments(&color_blend_attachment_states);

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .scissors(scissors)
            .viewports(viewports);
        let dynamic_state = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let color_attachment_refs = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let depth_attachment_ref = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };

        let subpass = vk::SubpassDescription::default()
            .color_attachments(&color_attachment_refs)
            .depth_stencil_attachment(&depth_attachment_ref)
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS);

        let renderpass_attachments = [
            vk::AttachmentDescription {
                format,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                store_op: vk::AttachmentStoreOp::STORE,
                initial_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                ..Default::default()
            },
            vk::AttachmentDescription {
                format: vk::Format::D16_UNORM,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                initial_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                ..Default::default()
            },
        ];

        let dependencies = [vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ
                | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ..Default::default()
        }];

        let renderpass_create_info = vk::RenderPassCreateInfo::default()
            .attachments(&renderpass_attachments)
            .subpasses(std::slice::from_ref(&subpass))
            .dependencies(&dependencies);

        let renderpass = unsafe { device.create_render_pass(&renderpass_create_info, None)? };

        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_state);

        let layout_create_info =
            vk::PipelineLayoutCreateInfo::default().push_constant_ranges(push_constant_ranges);

        let pipeline_layout = unsafe { device.create_pipeline_layout(&layout_create_info, None)? };
        Ok((
            unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(), // TODO:: create cache
                    &[vk::GraphicsPipelineCreateInfo::default()
                        .stages(&shader_stage_create_infos)
                        .vertex_input_state(&vertex_input_state_info)
                        .input_assembly_state(&vertex_input_assembly_state_info)
                        .viewport_state(&viewport_state_info)
                        .rasterization_state(&rasterization_info)
                        .multisample_state(&multisample_state_info)
                        .depth_stencil_state(&depth_state_info)
                        .color_blend_state(&color_blend_state)
                        .dynamic_state(&dynamic_state_info)
                        .layout(pipeline_layout)
                        .render_pass(renderpass)],
                    None,
                )
            }
            .map_err(|(_pipes, err)| err)?[0],
            renderpass,
            pipeline_layout,
        ))
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
