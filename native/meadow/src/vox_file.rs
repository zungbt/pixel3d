//! SPIKE: MagicaVoxel .vox loader and palette snapping. Plan: docs/superpowers/plans/2026-11-04-voxel-asset-spike.md, Task 2.
use crate::voxel::Model;

// SPIKE: Meadow palette subset for snapping .vox assets to the scene palette
pub const MEADOW: [u32; 21] = [
    0xa4cc86, 0xecd8b8, 0x8d6d52, 0x589e63, 0x46864f, 0x6db374, 0xaaafc2, 0x3a4044, 0x23262b, 0xf3ead2,
    0x55663a, 0x8c9a80, 0x4f8a3a, 0x5a9a3c, 0x3d6e2a, 0x8cc060, 0xe6dca0, 0xd8667a, 0xc3a284, 0xa0703f,
    0x6b4a2b,
];

// SPIKE: Load a MagicaVoxel .vox file into one meadow Model per vox model.
pub fn load(bytes: &[u8]) -> Result<Vec<Model>, String> {
    let vox_data = dot_vox::load_bytes(bytes).map_err(|e| e.to_string())?;

    let mut models = Vec::with_capacity(vox_data.models.len());

    for vox_model in &vox_data.models {
        let sx = vox_model.size.x as usize;
        let sy = vox_model.size.y as usize;
        let sz = vox_model.size.z as usize;

        let valid_voxels: Vec<_> = vox_model
            .voxels
            .iter()
            .copied()
            .filter(|v| {
                (v.x as usize) < sx
                    && (v.y as usize) < sy
                    && (v.z as usize) < sz
                    && (v.i as usize) < vox_data.palette.len()
            })
            .collect();

        let mut used_indices: Vec<u8> = valid_voxels.iter().map(|v| v.i).collect();
        used_indices.sort_unstable();
        used_indices.dedup();

        let model_palette: Vec<u32> = used_indices
            .iter()
            .map(|&idx| {
                let col = vox_data.palette[idx as usize];
                ((col.r as u32) << 16) | ((col.g as u32) << 8) | (col.b as u32)
            })
            .collect();

        let mut model = Model::new([sx, sz, sy], model_palette);

        for v in valid_voxels {
            let compacted_idx = (used_indices.binary_search(&v.i).unwrap() + 1) as u8;
            let mx = v.x as usize;
            let my = v.z as usize;
            let mz = sy - 1 - v.y as usize;
            model.set(mx, my, mz, compacted_idx);
        }

        models.push(model);
    }

    Ok(models)
}

// SPIKE: Replace each palette colour with the nearest entry of `to` (squared distance on sRGB bytes).
pub fn snap(m: &mut Model, to: &[u32]) {
    if to.is_empty() {
        return;
    }
    for c in &mut m.palette {
        let r = ((*c >> 16) & 0xff) as i32;
        let g = ((*c >> 8) & 0xff) as i32;
        let b = (*c & 0xff) as i32;

        let mut best_target = to[0];
        let mut best_dist = i32::MAX;

        for &target in to {
            let tr = ((target >> 16) & 0xff) as i32;
            let tg = ((target >> 8) & 0xff) as i32;
            let tb = (target & 0xff) as i32;
            let dr = r - tr;
            let dg = g - tg;
            let db = b - tb;
            let dist = dr * dr + dg * dg + db * db;
            if dist < best_dist {
                best_dist = dist;
                best_target = target;
            }
        }
        *c = best_target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPIKE: hand-written .vox test fixture builder
    fn tiny_vox(size: [u32; 3], voxels: &[[u8; 4]], palette: &[(u8, u32)]) -> Vec<u8> {
        let mut size_chunk = Vec::with_capacity(12 + 12);
        size_chunk.extend_from_slice(b"SIZE");
        size_chunk.extend_from_slice(&12u32.to_le_bytes());
        size_chunk.extend_from_slice(&0u32.to_le_bytes());
        size_chunk.extend_from_slice(&size[0].to_le_bytes());
        size_chunk.extend_from_slice(&size[1].to_le_bytes());
        size_chunk.extend_from_slice(&size[2].to_le_bytes());

        let xyzi_content_len = 4 + 4 * voxels.len() as u32;
        let mut xyzi_chunk = Vec::with_capacity(12 + xyzi_content_len as usize);
        xyzi_chunk.extend_from_slice(b"XYZI");
        xyzi_chunk.extend_from_slice(&xyzi_content_len.to_le_bytes());
        xyzi_chunk.extend_from_slice(&0u32.to_le_bytes());
        xyzi_chunk.extend_from_slice(&(voxels.len() as u32).to_le_bytes());
        for v in voxels {
            xyzi_chunk.extend_from_slice(v);
        }

        let mut rgba_content = [0u8; 1024];
        for &(idx, hex) in palette {
            if idx >= 1 {
                let slot = (idx - 1) as usize;
                if slot < 256 {
                    rgba_content[slot * 4] = ((hex >> 16) & 0xff) as u8;
                    rgba_content[slot * 4 + 1] = ((hex >> 8) & 0xff) as u8;
                    rgba_content[slot * 4 + 2] = (hex & 0xff) as u8;
                    rgba_content[slot * 4 + 3] = 255;
                }
            }
        }
        let mut rgba_chunk = Vec::with_capacity(12 + 1024);
        rgba_chunk.extend_from_slice(b"RGBA");
        rgba_chunk.extend_from_slice(&1024u32.to_le_bytes());
        rgba_chunk.extend_from_slice(&0u32.to_le_bytes());
        rgba_chunk.extend_from_slice(&rgba_content);

        let children_len = (size_chunk.len() + xyzi_chunk.len() + rgba_chunk.len()) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"VOX ");
        out.extend_from_slice(&150u32.to_le_bytes());
        out.extend_from_slice(b"MAIN");
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&children_len.to_le_bytes());
        out.extend_from_slice(&size_chunk);
        out.extend_from_slice(&xyzi_chunk);
        out.extend_from_slice(&rgba_chunk);
        out
    }

    #[test]
    fn vox_axes_are_y_up_and_not_mirrored() {
        let bytes = tiny_vox(
            [3, 2, 4],
            &[[2, 0, 0, 1], [0, 1, 3, 2]],
            &[(1, 0xff0000), (2, 0x00ff00)],
        );
        let models = load(&bytes).expect("valid vox");
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.size, [3, 4, 2]);
        let c1 = m.get([2, 0, 1]);
        assert_ne!(c1, 0);
        assert_eq!(m.palette[c1 as usize - 1], 0xff0000);
        let c2 = m.get([0, 3, 0]);
        assert_ne!(c2, 0);
        assert_eq!(m.palette[c2 as usize - 1], 0x00ff00);
        assert_eq!(m.solid(), 2);
    }

    #[test]
    fn unused_palette_entries_make_no_mesh() {
        let bytes = tiny_vox([1, 1, 1], &[[0, 0, 0, 7]], &[(7, 0x123456)]);
        let models = load(&bytes).expect("valid vox");
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.palette.len(), 1);
        assert_eq!(crate::voxel::meshes(m, true).len(), 1);
    }

    #[test]
    fn snap_picks_the_nearest_meadow_colour() {
        let mut m = Model::new([1, 1, 1], vec![0x5aa065, 0x8a6a50]);
        snap(&mut m, &MEADOW);
        assert_eq!(m.palette, vec![0x589e63, 0x8d6d52]);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(load(b"not a vox").is_err());
    }
}
