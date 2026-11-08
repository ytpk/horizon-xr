// SPDX-License-Identifier: MPL-2.0
//! A Horizon client and the Horizon server half of `visor:channel`, in memory, through the
//! wire bytes both ways.

#![allow(clippy::field_reassign_with_default)]

use glam::{Quat, Vec3};
use horizon_visor::body::BodyType;
use horizon_visor::buffer::{PoseData, PoseElement};
use horizon_visor::client::{ClientNet, Event, LocalState};
use horizon_visor::payload::{C2s, S2c};
use horizon_visor::server::VisorServer;
use horizon_visor::settings::{ServerSettings, SupportedMovement};

fn local(y: f32) -> LocalState {
    let mut pose = PoseData::default();
    pose.hmd = PoseElement::new(Vec3::new(0.0, y, 0.0), Quat::IDENTITY);
    LocalState {
        height: 1.52,
        world_scale: 1.0,
        rotation_y: 0.0,
        left_handed: false,
        active_hand_main: true,
        offhand_slot: -1,
        gun_angle: 60.0,
        body_type: BodyType::FullBody,
        overlay_focused: false,
        pose,
    }
}

/// Client `uuid` says HANDSHAKE and takes every answer.
fn join(server: &mut VisorServer, client: &mut ClientNet, uuid: u128, vr: bool) -> Vec<Event> {
    let hello = client.login(vr).write();
    let out = server.handle(uuid, C2s::read(&hello).unwrap());
    out.replies
        .iter()
        .filter_map(|r| client.handle(S2c::read(&r.write()).unwrap()))
        .collect()
}

/// A tick: the client's updates to the server over the wire.
fn send(server: &mut VisorServer, client: &mut ClientNet, uuid: u128, state: &LocalState) {
    for p in client.state_updates(state) {
        server.handle(uuid, C2s::read(&p.write()).unwrap());
    }
}

#[test]
fn two_clients_meet_through_the_server() {
    let mut settings = ServerSettings::default();
    settings.supported_movement = SupportedMovement::Teleport;
    settings.teleport_forward_limit = 7;
    let mut server = VisorServer::new(settings);
    let (mut a, mut b) = (ClientNet::default(), ClientNet::default());

    // The first client joins: the server's settings replace the vanilla profile.
    let events = join(&mut server, &mut a, 1, true);
    assert!(events.contains(&Event::ServerSupportsVisor));
    assert!(events.contains(&Event::SettingsChanged));
    assert!(events.contains(&Event::OffhandSlot(0)));
    assert_eq!(a.settings.supported_movement, SupportedMovement::Teleport);
    assert_eq!(a.settings.teleport_forward_limit, 7);
    join(&mut server, &mut b, 2, true);

    // A ticks; B is within sight of A.
    send(&mut server, &mut a, 1, &local(1.7));
    let heard = server.state_updates(1, &[2]);
    assert_eq!(heard.len(), 1);
    for (to, p) in heard {
        assert_eq!(to, 2);
        b.handle(S2c::read(&p.write()).unwrap());
    }
    assert_eq!(b.players[&1].body_type, BodyType::FullBody);
    assert_eq!(b.players[&1].pose.hmd.position.y, 1.7);

    // A crouches (the pose changes) and turns left handed: B sees both.
    let mut crouched = local(1.1);
    crouched.left_handed = true;
    send(&mut server, &mut a, 1, &crouched);
    for (_, p) in server.state_updates(1, &[2]) {
        b.handle(S2c::read(&p.write()).unwrap());
    }
    assert!(b.players[&1].left_handed);
    assert_eq!(b.players[&1].pose.hmd.position.y, 1.1);
    assert_eq!(b.players[&1].prev_pose.hmd.position.y, 1.7);
}

#[test]
fn a_plain_server_never_answers_so_the_client_stays_silent() {
    let mut a = ClientNet::default();
    a.login(true);
    assert!(a.state_updates(&local(1.7)).is_empty());
    assert!(!a.server_supports_visor);
    // The vanilla profile stays: no better swinging, controller movement.
    assert!(!a.settings.better_swinging);
    assert_eq!(a.settings.supported_movement, SupportedMovement::Controller);
}
