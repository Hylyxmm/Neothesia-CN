use wgpu::vertex_attr_array;
use wgpu_jumpstart::wgpu;

use bytemuck::{Pod, Zeroable};

/// One soft light shape. Coordinates and sizes are logical px; the vertical fade, horizontal
/// feather and radial modes are resolved per-pixel in the fragment shader (see shader.wgsl).
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable, PartialEq)]
pub struct LightInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    /// x: fade_h (upward fade height, 0 = plain), y: mode (0 = rect, 1 = radial disc),
    /// z: horizontal feather fraction, w: unused.
    pub params: [f32; 4],
}

impl Default for LightInstance {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            size: [0.0, 0.0],
            color: [0.0, 0.0, 0.0, 1.0],
            params: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

impl LightInstance {
    pub fn attributes() -> [wgpu::VertexAttribute; 4] {
        vertex_attr_array!(1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4)
    }

    pub fn layout(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LightInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        }
    }
}
