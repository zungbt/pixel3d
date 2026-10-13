//! The fixed orthographic view (as the web's main.js) and the sun's shadow camera.
use glam::camera::rh::proj::directx::orthographic; // wgpu NDC: y up, depth 0..1
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec2, Vec3};

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
#[cfg(test)]
pub fn half_width(art_w: u32, art_h: u32) -> f32 {
    VIEW_HEIGHT / 2.0 * art_w as f32 / art_h as f32
}

/// One screen's part of the view: its art grid's centre in view space (the primary's is the
/// origin) and the world units per art pixel (the primary's VIEW_HEIGHT over its art height), so
/// screens side by side show one meadow running across the bezels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub centre: Vec2,
    pub unit: f32,
}

impl Pane {
    /// A screen of art size `art` whose top-left corner sits `offset` screen px from the primary's,
    /// whose art size is `primary`.
    pub fn new(art: (u32, u32), offset: (i32, i32), primary: (u32, u32), pixel_size: u32) -> Self {
        let unit = VIEW_HEIGHT / primary.1 as f32;
        let px = pixel_size as f32;
        let x = offset.0 as f32 / px + (art.0 as f32 - primary.0 as f32) / 2.0;
        let y = offset.1 as f32 / px + (art.1 as f32 - primary.1 as f32) / 2.0;
        Self { centre: Vec2::new(x, -y) * unit, unit } // screen y runs down, view y up
    }

    pub fn half(&self, art: (u32, u32)) -> Vec2 {
        Vec2::new(art.0 as f32, art.1 as f32) * self.unit / 2.0
    }

    /// Where the pane's centre meets the ground plane (y = 0): the middle of its shadow camera.
    pub fn ground_centre(&self) -> Vec3 {
        ground_at(self.centre)
    }
}

/// The ground plane (y = 0) under a view-space point.
pub fn ground_at(p: Vec2) -> Vec3 {
    let inv = view().inverse();
    let o = inv.transform_point3(p.extend(0.0));
    let forward = inv.transform_vector3(Vec3::NEG_Z);
    o + forward * (-o.y / forward.y)
}

pub fn projection(art: (u32, u32), pane: Pane) -> Mat4 {
    let (c, half) = (pane.centre, pane.half(art));
    // Near/far as the web: orthographic depth is linear, so the outline thresholds carry over. A
    // pane below the primary sees ground nearer the eye: its range moves with its ground centre.
    let shift = -view().transform_point3(pane.ground_centre()).z - DIST;
    orthographic(c.x - half.x, c.x + half.x, c.y - half.y, c.y + half.y, 0.1 + shift, 100.0 + shift)
}

/// The sun's (or moon's) shadow camera, as the web's: ±22 box, near/far 1..80, moved along with a
/// pane to `centre` (the origin for the primary). The light's z is always 13.68
/// (meadow_core::sky), so it never sits on the y axis and look_at with +Y is defined.
pub fn light_view_proj(sun: Vec3, centre: Vec3) -> Mat4 {
    let proj = orthographic(-SHADOW_HALF, SHADOW_HALF, -SHADOW_HALF, SHADOW_HALF, 1.0, 80.0);
    proj * look_at_mat4(sun + centre, centre, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meadow_core::sky;

    // Where the corners of a 21:9 screen (the widest the grass culling plans for) meet y = 0.
    fn ground_corners() -> Vec<Vec3> {
        [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(sx, sy)| ground_at(Vec2::new(sx * half_width(21, 9), sy * VIEW_HEIGHT / 2.0)))
            .collect()
    }

    #[test]
    fn the_primary_pane_is_the_single_screen_view() {
        for art in [(854, 480), (640, 360), (480, 360), (1120, 480)] {
            let pane = Pane::new(art, (0, 0), art, 3);
            let (hw, hh) = (half_width(art.0, art.1), VIEW_HEIGHT / 2.0);
            assert!(projection(art, pane).abs_diff_eq(orthographic(-hw, hw, -hh, hh, 0.1, 100.0), 1e-5), "{art:?}");
            assert!(pane.ground_centre().length() < 1e-4, "{art:?}");
        }
    }

    #[test]
    fn neighbouring_screens_continue_the_view_pixel_for_pixel() {
        // 2560x1440 at pixel size 3 (art 854x480, cropped right) with a 1920x1080 (art 640x360)
        // below it 339 px right, or beside it. Each art grid starts at its window's top-left
        // corner, so a screen `offset` px away starts offset / 3 art px further along the view.
        let (primary, other) = ((854, 480), (640, 360));
        let top = Pane::new(primary, (0, 0), primary, 3);
        let corner = |p: Pane, art| Vec2::new(p.centre.x - p.half(art).x, p.centre.y + p.half(art).y);
        for offset in [(339, 1440), (2560, 0), (-1920, 200)] {
            let pane = Pane::new(other, offset, primary, 3);
            assert_eq!(pane.unit, top.unit, "same scale on every screen");
            let want = corner(top, primary) + Vec2::new(offset.0 as f32, -offset.1 as f32) / 3.0 * top.unit;
            assert!(corner(pane, other).abs_diff_eq(want, 1e-4), "{offset:?}: {} vs {want}", corner(pane, other));
        }
    }

    #[test]
    fn every_pane_sees_its_ground_between_near_and_far() {
        // The 1920x1080 below the 2560x1440 (pixel size 3) once lost its lower third to the near plane.
        let primary = (854, 480);
        for (art, offset) in [(primary, (0, 0)), ((640, 360), (339, 1440)), ((640, 360), (339, -1080)), ((360, 640), (2560, 0))] {
            let pane = Pane::new(art, offset, primary, 3);
            let (proj, h) = (projection(art, pane) * view(), pane.half(art));
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)] {
                let z = proj.project_point3(ground_at(pane.centre + Vec2::new(sx * h.x, sy * h.y))).z;
                assert!((0.05..0.95).contains(&z), "{offset:?} corner {sx},{sy}: depth {z}");
            }
        }
    }

    #[test]
    fn visible_ground_stays_between_the_shadow_cameras_near_and_far_planes() {
        // The corners land ~27.7 units out: past the ±22 box sideways (as on the web), but every
        // depth must stay in 1..80, or the ground there would shadow or light wrongly.
        let corners = ground_corners();
        assert!(corners.iter().all(|c| (27.0..28.5).contains(&c.x.hypot(c.z))), "{corners:?}");
        for day_t in 0..300 {
            let p = sky::sky_state(day_t as f64, 0.0).sun_pos;
            let light = light_view_proj(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), Vec3::ZERO);
            for c in &corners {
                let z = light.project_point3(*c).z;
                assert!((0.05..0.95).contains(&z), "day_t {day_t}: corner {c} at shadow depth {z}");
            }
        }
    }
}
