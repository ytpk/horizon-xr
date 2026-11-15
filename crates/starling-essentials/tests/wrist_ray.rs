// SPDX-License-Identifier: LGPL-3.0-only
//! Ray -> upstream wrist placement -> menu slot ID, including the tall book canvas.
use glam::{Quat, Vec3};
use starling_essentials::inventory::{self, hit};

#[test]
fn either_size_and_world_scale_target_the_same_authoritative_slots() {
    let canvas = [426.0, 536.0];
    for world_scale in [0.25, 1.0, 2.0] {
        for size in [0.25, 0.5, 1.5] {
            let (pos, rot) = inventory::wrist(
                Vec3::new(-0.25, 0.1, -0.3),
                Quat::from_rotation_y(0.4),
                world_scale,
            );
            let width = canvas[0] / 427.0 * size * world_scale;
            let height = canvas[1] / 427.0 * size * world_scale;
            // Independent fixtures from VRInvScreen.java, not calculated by inventory::slot.
            for (id, [x, y]) in [
                (0, [149.0, 46.0]),
                (9, [77.0, 104.0]),
                (35, [221.0, 140.0]),
                (504, [87.0, 70.0]),
                (200, [265.0, 44.0]),
            ] {
                let x = x + (canvas[0] - 278.0) * 0.5;
                let y = y + (canvas[1] - 156.0) * 0.5;
                let surface = pos
                    + rot
                        * Vec3::new(
                            (x / canvas[0] - 0.5) * width,
                            (0.5 - y / canvas[1]) * height,
                            0.0,
                        );
                let normal = rot * Vec3::Z;
                let ([u, v], distance) = horizon_visor::keyboard::ray_uv(
                    surface + normal * 0.3,
                    -normal,
                    pos,
                    rot,
                    width,
                    height,
                )
                .unwrap();
                assert!((distance - 0.3).abs() < 1e-5);
                let local_x = u * canvas[0] - (canvas[0] - 278.0) * 0.5;
                let local_y = v * canvas[1] - (canvas[1] - 156.0) * 0.5;
                assert_eq!(
                    hit(local_x, local_y, true, true),
                    Some(id),
                    "scale={world_scale},size={size}"
                );
                if id >= 100 {
                    assert_eq!(hit(local_x, local_y, false, true), None);
                }
            }
        }
    }
}

#[test]
fn default_pose_uses_visors_zyx_offset_order_not_yxz() {
    let (at, rot) = inventory::wrist(Vec3::ZERO, Quat::IDENTITY, 1.0);
    assert!((at - Vec3::new(-0.07, -0.081, 0.2)).length() < 1e-6);
    assert!((rot * Vec3::Z - Vec3::NEG_X).length() < 1e-6);
    assert!((rot * Vec3::Y - Vec3::NEG_Y).length() < 1e-6);
}
