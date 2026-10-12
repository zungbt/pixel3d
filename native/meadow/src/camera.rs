//! The fixed orthographic view (as the web's main.js) and the sun's shadow camera.
use glam::camera::rh::proj::directx::orthographic; // wgpu NDC: y up, depth 0..1
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec3};

pub const VIEW_HEIGHT: f32 = 18.0; // world units visible vertically
const YAW: f32 = std::f32::consts::FRAC_PI_4;
const PITCH: f32 = 30.0 * std::f32::consts::PI / 180.0;
const DIST: f32 = 30.0;

const SHADOW_HALF: f32 = 22.0; // as the web; the 21:9 screen corners land ~27.7 units out, past it (no shadows there)

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

/// The sun's (or moon's) shadow camera, as the web's: ±22 box, near/far 1..80. The light's z is
/// always 13.68 (meadow_core::sky), so it never sits on the y axis and look_at with +Y is defined.
pub fn light_view_proj(sun: Vec3) -> Mat4 {
    let proj = orthographic(-SHADOW_HALF, SHADOW_HALF, -SHADOW_HALF, SHADOW_HALF, 1.0, 80.0);
    proj * look_at_mat4(sun, Vec3::ZERO, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meadow_core::sky;

    // Where the corners of a 21:9 screen (the widest the grass culling plans for) meet y = 0.
    fn ground_corners() -> Vec<Vec3> {
        let inv = view().inverse();
        let forward = inv.transform_vector3(Vec3::NEG_Z);
        [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(sx, sy)| {
                let o = inv.transform_point3(Vec3::new(sx * half_width(21, 9), sy * VIEW_HEIGHT / 2.0, 0.0));
                o + forward * (-o.y / forward.y)
            })
            .collect()
    }

    #[test]
    fn visible_ground_stays_between_the_shadow_cameras_near_and_far_planes() {
        // The corners land ~27.7 units out: past the ±22 box sideways (as on the web), but every
        // depth must stay in 1..80, or the ground there would shadow or light wrongly.
        let corners = ground_corners();
        assert!(corners.iter().all(|c| (27.0..28.5).contains(&c.x.hypot(c.z))), "{corners:?}");
        for day_t in 0..300 {
            let p = sky::sky_state(day_t as f64, 0.0).sun_pos;
            let light = light_view_proj(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
            for c in &corners {
                let z = light.project_point3(*c).z;
                assert!((0.05..0.95).contains(&z), "day_t {day_t}: corner {c} at shadow depth {z}");
            }
        }
    }
}
