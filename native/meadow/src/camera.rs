//! The fixed orthographic view (as the web's main.js) and the sun's shadow camera.
use glam::camera::rh::proj::directx::orthographic; // wgpu NDC: y up, depth 0..1
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec3};

pub const VIEW_HEIGHT: f32 = 18.0; // world units visible vertically
const YAW: f32 = std::f32::consts::FRAC_PI_4;
const PITCH: f32 = 30.0 * std::f32::consts::PI / 180.0;
const DIST: f32 = 30.0;

/// Noon sun, as the web's sky at elevation 1.
pub const SUN_POS: Vec3 = Vec3::new(0.0, 38.0, 38.0 * 0.36);
const SHADOW_HALF: f32 = 22.0; // covers the visible ground (screen corners land ~20 units out on 21:9)

pub fn view() -> Mat4 {
    let eye = DIST * Vec3::new(PITCH.cos() * YAW.sin(), PITCH.sin(), PITCH.cos() * YAW.cos());
    look_at_mat4(eye, Vec3::ZERO, Vec3::Y)
}

/// Half the visible width in world units. Aspect from the art size, not the window, so every art
/// pixel is square.
pub fn half_width(art_w: u32, art_h: u32) -> f32 {
    VIEW_HEIGHT / 2.0 * art_w as f32 / art_h as f32
}

pub fn projection(art_w: u32, art_h: u32) -> Mat4 {
    let half_h = VIEW_HEIGHT / 2.0;
    let half_w = half_width(art_w, art_h);
    // Near/far as the web: orthographic depth is linear, so the outline thresholds carry over.
    orthographic(-half_w, half_w, -half_h, half_h, 0.1, 100.0)
}

pub fn light_view_proj() -> Mat4 {
    let proj = orthographic(-SHADOW_HALF, SHADOW_HALF, -SHADOW_HALF, SHADOW_HALF, 1.0, 80.0);
    proj * look_at_mat4(SUN_POS, Vec3::ZERO, Vec3::Y)
}
