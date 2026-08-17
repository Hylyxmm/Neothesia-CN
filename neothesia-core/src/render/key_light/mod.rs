mod instance_data;
pub use instance_data::LightInstance;

use wgpu_jumpstart::{Gpu, Instances, Shape, TransformUniform, Uniform, wgpu};

/// Renders translucent light shapes with per-pixel soft gradients (vertical fade, horizontal
/// feather, radial discs) — used by the key-light strip at the bottom of the playing scene.
/// Unlike stacked translucent quads, the fades are computed in the fragment shader, so there is
/// no visible banding.
#[derive(Clone)]
struct LightPipeline {
    render_pipeline: wgpu::RenderPipeline,
    transform_uniform_bind_group: wgpu::BindGroup,
    quad: Shape,
}

impl LightPipeline {
    fn new(gpu: &Gpu, transform_uniform: &Uniform<TransformUniform>) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("KeyLightPipeline::shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                    "./shader.wgsl"
                ))),
            });

        let render_pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&transform_uniform.bind_group_layout)],
                immediate_size: 0,
            });

        let target = wgpu_jumpstart::default_color_target_state(gpu.texture_format);

        let render_pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                layout: Some(&render_pipeline_layout),
                fragment: Some(wgpu_jumpstart::default_fragment(&shader, &[Some(target)])),
                ..wgpu_jumpstart::default_render_pipeline(wgpu_jumpstart::default_vertex(
                    &shader,
                    &[
                        Some(Shape::layout()),
                        Some(LightInstance::layout(&LightInstance::attributes())),
                    ],
                ))
            });

        Self {
            render_pipeline,
            transform_uniform_bind_group: transform_uniform.bind_group.clone(),
            quad: Shape::new_quad(&gpu.device),
        }
    }

    #[profiling::function]
    fn render<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        lights: &Instances<LightInstance>,
    ) {
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.transform_uniform_bind_group, &[]);

        render_pass.set_vertex_buffer(0, self.quad.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, lights.buffer.slice(..));

        render_pass.set_index_buffer(self.quad.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        render_pass.draw_indexed(0..self.quad.indices_len, 0, 0..lights.len());
    }
}

pub struct LightRenderer {
    pipeline: LightPipeline,
    lights: Instances<LightInstance>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl LightRenderer {
    pub fn new(gpu: &Gpu, transform_uniform: &Uniform<TransformUniform>) -> Self {
        Self {
            pipeline: LightPipeline::new(gpu, transform_uniform),
            lights: Instances::new(&gpu.device, 100),
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
        }
    }

    pub fn clear(&mut self) {
        self.lights.data.clear();
    }

    pub fn layer(&mut self) -> &mut Vec<LightInstance> {
        &mut self.lights.data
    }

    pub fn push(&mut self, light: LightInstance) {
        self.lights.data.push(light)
    }

    #[profiling::function]
    pub fn prepare(&mut self) {
        self.lights.update(&self.device, &self.queue);
    }

    #[profiling::function]
    pub fn render<'a>(&'a self, render_pass: &mut wgpu_jumpstart::RenderPass<'a>) {
        self.pipeline.render(render_pass, &self.lights);
    }
}
