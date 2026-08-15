#![allow(clippy::single_match)]

mod error;
use std::ops::{Deref, DerefMut};

pub use error::GpuInitError;

mod color;
mod gpu;
mod instances;
mod render_pipeline_builder;
mod shape;
mod uniform;

mod transform_uniform;

pub use color::Color;
pub use gpu::{Gpu, Surface};
pub use instances::Instances;
pub use render_pipeline_builder::{
    default_color_target_state, default_fragment, default_render_pipeline, default_vertex,
};
pub use shape::Shape;
pub use transform_uniform::TransformUniform;
pub use uniform::Uniform;
pub use wgpu;

pub struct RenderPass<'a>(wgpu::RenderPass<'a>, wgpu::Extent3d);

impl<'a> RenderPass<'a> {
    pub fn new(rpass: wgpu::RenderPass<'a>, size: wgpu::Extent3d) -> Self {
        Self(rpass, size)
    }

    pub fn size(&self) -> wgpu::Extent3d {
        self.1
    }

    /// Clamping wrapper: during DPI/scale transitions (window moved between monitors,
    /// exclusive fullscreen mode switches) the tracked logical size can briefly disagree
    /// with the swapchain, producing rects outside the render target — which wgpu treats
    /// as a fatal validation error. Clamp to the target and survive the transient state.
    /// (Inherent methods resolve before the `Deref` impl, so every caller gets this.)
    pub fn set_scissor_rect(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let w = w.min(self.1.width.saturating_sub(x));
        let h = h.min(self.1.height.saturating_sub(y));
        self.0.set_scissor_rect(x, y, w, h);
    }
}

impl<'a> Deref for RenderPass<'a> {
    type Target = wgpu::RenderPass<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for RenderPass<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
