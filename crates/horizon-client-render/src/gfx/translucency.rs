//! Vanilla 26.3 MeshData.SortState / VertexSorting.byDistance.
//! Keep vertex order intact; only the quad indices change as the camera moves.

/// MeshData.decodeQuadCentroids uses the opposite corners, not all four vertices.
pub fn quad_center(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| (a[i] + b[i]) / 2.0)
}

#[derive(Default)]
pub struct Sorter {
    order: Vec<(u32, f32)>,
    indices: Vec<u32>,
}

impl Sorter {
    pub fn sort(&mut self, centers: &[[f32; 3]], camera: [f32; 3]) -> &[u32] {
        self.order.clear();
        self.order.extend(centers.iter().enumerate().map(|(i, c)| {
            let [x, y, z] = std::array::from_fn(|j| camera[j] - c[j]);
            (i as u32, x * x + y * y + z * z)
        }));
        // VertexSorting uses a stable merge sort, preserving mesh order for ties.
        self.order.sort_by(|a, b| b.1.total_cmp(&a.1));
        self.indices.clear();
        for &(quad, _) in &self.order {
            let v = quad * 4;
            self.indices.extend([v, v + 1, v + 2, v + 2, v + 3, v]);
        }
        &self.indices
    }
}

pub struct SortState {
    centers: Vec<[f32; 3]>,
    last: Option<([i32; 3], [i32; 3])>,
    bounds: Option<[glam::Vec3; 2]>,
    plane: Option<super::planar_water::Plane>,
    facing: Option<u8>,
    index_count: u32,
}

impl SortState {
    pub fn new(centers: Vec<[f32; 3]>) -> Self {
        let index_count = centers.len() as u32 * 6;
        Self {
            centers,
            last: None,
            bounds: None,
            plane: None,
            facing: None,
            index_count,
        }
    }

    pub fn from_vertices(vertices: &[crate::mesher::TerrainVertex], headset: bool) -> Self {
        let mut state = Self::new(
            vertices
                .as_chunks::<4>()
                .0
                .iter()
                .map(|q| quad_center(q[0].pos, q[2].pos))
                .collect(),
        );
        if headset
            && !vertices.is_empty()
            && vertices.iter().all(|v| v.pos.iter().all(|f| f.is_finite()))
        {
            let mut low = glam::Vec3::splat(f32::INFINITY);
            let mut high = glam::Vec3::splat(f32::NEG_INFINITY);
            for v in vertices {
                let p = glam::Vec3::from(v.pos);
                low = low.min(p);
                high = high.max(p);
            }
            let pad = glam::Vec3::splat(crate::mesher::SectionMesh::BOUNDS_PAD);
            state.bounds = Some([low - pad, high + pad]);
            state.plane =
                super::planar_water::classify(&vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
        }
        state
    }

    pub fn visible(&self, origin: glam::DVec3, frustum: &super::Frustum) -> bool {
        self.bounds.is_none_or(|[low, high]| {
            frustum.intersects(origin.as_vec3() + low, origin.as_vec3() + high)
        })
    }

    pub fn index_count(&self) -> u32 {
        self.index_count
    }
    pub fn needs_headset_update(&self, enabled: bool) -> bool {
        self.facing.is_some() || enabled && self.plane.is_some()
    }

    /// Disjoint coplanar faces cannot blend over each other. Their original
    /// order is enough; omit only windings culled for *every* current eye.
    pub fn update_headset<'a>(
        &mut self,
        camera: [f64; 3],
        origin: [i32; 3],
        eye_y: [f64; 2],
        sorter: &'a mut Sorter,
        lod: bool,
    ) -> Option<&'a [u32]> {
        if let Some(plane) = &self.plane {
            let y = f64::from(origin[1]) + f64::from(plane.y);
            let mask = if eye_y.iter().all(|v| *v > y + 0.0001) {
                1
            } else if eye_y.iter().all(|v| *v < y - 0.0001) {
                2
            } else {
                3
            };
            if self.facing == Some(mask) {
                return None;
            }
            self.facing = Some(mask);
            self.last = Some((camera.map(|v| v.floor() as i32), [0; 3]));
            sorter.indices.clear();
            for (q, &up) in plane.up.iter().enumerate() {
                if mask & if up { 1 } else { 2 } != 0 {
                    let v = q as u32 * 4;
                    sorter.indices.extend([v, v + 1, v + 2, v + 2, v + 3, v]);
                }
            }
            self.index_count = sorter.indices.len() as u32;
            return Some(&sorter.indices);
        }
        if lod {
            self.update_lod(camera, origin, sorter)
        } else {
            self.update(camera, origin, sorter)
        }
    }

    fn restore_regular(&mut self) {
        if self.facing.take().is_some() {
            self.last = None;
        }
        self.index_count = self.centers.len() as u32 * 6;
    }

    pub fn is_sorted(&self) -> bool {
        self.last.is_some()
    }

    /// Large tiles cannot use the section viewpoint shortcut: the camera may
    /// cross a quad's plane while remaining on the same side of the first 16³.
    pub fn update_lod<'a>(
        &mut self,
        camera: [f64; 3],
        origin: [i32; 3],
        sorter: &'a mut Sorter,
    ) -> Option<&'a [u32]> {
        self.restore_regular();
        let block = camera.map(|v| v.floor() as i32);
        if self.last.is_some_and(|(last, _)| last == block) {
            return None;
        }
        self.last = Some((block, [0; 3]));
        Some(sorter.sort(
            &self.centers,
            std::array::from_fn(|i| (camera[i] - f64::from(origin[i])) as f32),
        ))
    }

    /// TranslucencyPointOfView and ResortTransparencyTask: diagonal views keep
    /// their ordering until the point of view changes; aligned views also
    /// resort on crossing a block boundary. Sub-block movement needs no upload.
    pub fn update<'a>(
        &mut self,
        camera: [f64; 3],
        origin: [i32; 3],
        sorter: &'a mut Sorter,
    ) -> Option<&'a [u32]> {
        self.restore_regular();
        let block = camera.map(|v| v.floor() as i32);
        let pov = std::array::from_fn(|i| ((block[i] >> 4) - (origin[i] >> 4)).clamp(-1, 1));
        if let Some((last_block, last_pov)) = self.last
            && last_pov == pov
            && (last_block == block || !pov.contains(&0))
        {
            return None;
        }
        self.last = Some((block, pov));
        Some(sorter.sort(
            &self.centers,
            std::array::from_fn(|i| (camera[i] - f64::from(origin[i])) as f32),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane_mesh() -> Vec<crate::mesher::TerrainVertex> {
        let q = [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ];
        q.into_iter()
            .chain(q.into_iter().rev())
            .map(|p| crate::mesher::TerrainVertex::new(p, [0.0; 2], 0, 0, 0))
            .collect()
    }

    #[test]
    fn visible_flat_water_keeps_only_front_faces_and_needs_no_movement_uploads() {
        let mut state = SortState::from_vertices(&plane_mesh(), true);
        let mut sorter = Sorter::default();
        assert_eq!(
            state
                .update_headset([0.5, 3.0, 0.5], [0; 3], [3.0; 2], &mut sorter, false)
                .unwrap(),
            &[0, 1, 2, 2, 3, 0]
        );
        for x in -100..100 {
            assert!(
                state
                    .update_headset(
                        [f64::from(x), 3.0, 0.5],
                        [0; 3],
                        [3.0; 2],
                        &mut sorter,
                        true
                    )
                    .is_none()
            );
        }
        assert_eq!(state.index_count(), 6);
        assert_eq!(
            state
                .update_headset([0.5, 0.0, 0.5], [0; 3], [0.0; 2], &mut sorter, false)
                .unwrap(),
            &[4, 5, 6, 6, 7, 4]
        );
        assert_eq!(
            state
                .update_headset([0.5, 1.0, 0.5], [0; 3], [0.99, 1.01], &mut sorter, false)
                .unwrap()
                .len(),
            12
        );
    }

    #[test]
    fn one_sided_water_can_have_zero_indices_and_recover_after_crossing() {
        let mesh = plane_mesh();
        let mut state = SortState::from_vertices(&mesh[..4], true);
        let mut sorter = Sorter::default();
        assert!(
            state
                .update_headset([0.0; 3], [0; 3], [0.0; 2], &mut sorter, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.index_count(), 0);
        assert_eq!(
            state
                .update_headset([0.0, 3.0, 0.0], [0; 3], [3.0; 2], &mut sorter, false)
                .unwrap()
                .len(),
            6
        );
        assert_eq!(state.index_count(), 6);
    }

    #[test]
    fn switching_to_regular_or_shader_pack_rendering_restores_all_indices() {
        let mut state = SortState::from_vertices(&plane_mesh(), true);
        let mut sorter = Sorter::default();
        state.update_headset([0.5, 3.0, 0.5], [0; 3], [3.0; 2], &mut sorter, false);
        assert!(state.needs_headset_update(false));
        assert_eq!(
            state
                .update([0.5, 3.0, 0.5], [0; 3], &mut sorter)
                .unwrap()
                .len(),
            12
        );
        assert_eq!(state.index_count(), 12);
        assert!(!state.needs_headset_update(false));
    }

    #[test]
    fn transparent_bounds_can_reject_water_below_a_visible_section() {
        let state = SortState::from_vertices(&plane_mesh(), true);
        // Looking above y=4: the section extends into view but its water does not.
        let f = super::super::Frustum {
            planes: [glam::Vec4::new(0.0, 1.0, 0.0, -4.0); 6],
        };
        assert!(f.intersects(glam::Vec3::ZERO, glam::Vec3::splat(16.0)));
        assert!(!state.visible(glam::DVec3::ZERO, &f));
        assert!(state.visible(glam::DVec3::new(0.0, 3.0, 0.0), &f));
        assert!(SortState::new(vec![]).visible(glam::DVec3::ZERO, &f));
    }

    #[test]
    fn equal_distances_keep_mesh_order_and_triangle_winding() {
        let mut sorter = Sorter::default();
        assert_eq!(
            sorter.sort(
                &[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [4.0, 0.0, 0.0]],
                [0.0; 3]
            ),
            &[8, 9, 10, 10, 11, 8, 0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4]
        );
        assert!(sorter.sort(&[], [0.0; 3]).is_empty());
    }

    #[test]
    fn movement_reverses_faces_without_moving_vertices() {
        let centers = [[0.5, 0.5, 0.0], [0.5, 0.5, 1.0]];
        let mut sorter = Sorter::default();
        assert_eq!(sorter.sort(&centers, [0.5, 0.5, -2.0])[0], 4);
        assert_eq!(sorter.sort(&centers, [0.5, 0.5, 3.0])[0], 0);
    }

    #[test]
    fn cached_viewpoint_handles_negative_and_large_coordinates() {
        let origin = [-30_000_000, -64, 30_000_000];
        let mut state = SortState::new(vec![[0.5; 3]]);
        let mut sorter = Sorter::default();
        let camera = [-29_999_999.5, -63.5, 30_000_000.5];
        assert!(state.update(camera, origin, &mut sorter).is_some());
        assert!(state.update(camera, origin, &mut sorter).is_none());
        assert!(
            state
                .update([-29_999_999.1, -63.1, 30_000_000.9], origin, &mut sorter)
                .is_none()
        );
        assert!(
            state
                .update([-29_999_998.9, -63.1, 30_000_000.9], origin, &mut sorter)
                .is_some()
        );
    }
    #[test]
    fn large_tiles_resort_inside_the_same_section_viewpoint() {
        let mut state = SortState::new(vec![[32.0, 0.0, 0.0], [96.0, 0.0, 0.0]]);
        let mut sorter = Sorter::default();
        assert_eq!(
            state
                .update_lod([48.0, 20.0, 20.0], [0; 3], &mut sorter)
                .unwrap()[0],
            4
        );
        assert!(
            state
                .update_lod([48.5, 20.0, 20.0], [0; 3], &mut sorter)
                .is_none()
        );
        // Both views have positive x/y/z signs relative to the first section,
        // so the section-sized shortcut would incorrectly keep the old order.
        assert_eq!(
            state
                .update_lod([80.0, 20.0, 20.0], [0; 3], &mut sorter)
                .unwrap()[0],
            0
        );
    }
}
