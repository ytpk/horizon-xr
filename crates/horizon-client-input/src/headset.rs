//! A VR headset's input, shared between the embedder that runs the headset's
//! session (horizon-xr on Meta Quest) and the game. The headset side writes
//! what the controllers and the head did; the game reads it once per loop
//! iteration, as it reads a controller, and writes back what the headset
//! needs to know (a menu is open, the body's yaw).
//!
//! Nothing here is set on a flat screen: every reader then sees `None` or
//! `false` and the game behaves as it always did.
use crate::gamepad::PadState;
use glam::{Quat, Vec3};
use std::sync::Mutex;

pub use horizon_vivecraft::modes::{FreeMove, HudLock, PointerElement};
pub use horizon_vivecraft::settings::VrSettings;
use std::sync::Arc;

/// A tracked controller at one moment, in the play space (metres, +Y up, -Z
/// ahead) relative to the head's rest position (the game's camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPose {
    /// The hand holding the controller: its middle and turn (`grip/pose`).
    pub grip: (Vec3, Quat),
    /// The controller's pointing ray: where it starts and its turn (-Z along
    /// the ray, `aim/pose`).
    pub aim: (Vec3, Quat),
}

/// How many second panels the game can show beside the HUD: the hand's overlay (the hotbar wheel,
/// the movement state), the chat, the handles of a screen, the keyboard and the off-hand wheel.
pub const OVERLAYS: usize = 5;
/// The overlay slots.
pub const OVERLAY_HAND: usize = 0;
pub const OVERLAY_CHAT: usize = 1;
pub const OVERLAY_HANDLES: usize = 2;
/// Visor's keyboard (`VROverlayKeyboard`), under a screen or alone.
pub const OVERLAY_KEYBOARD: usize = 3;
/// The off-hand hotbar wheel has its own panel so both grips can be held.
pub const OVERLAY_OFFHAND: usize = 4;

/// The handles below a screen's panel and what is going on with them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenHandles {
    /// The panel can be dragged and resized: its handles show.
    pub visible: bool,
    pub dragging: bool,
    pub resizing: bool,
}

/// A second panel in the play space beside the HUD (`VROverlay`s that hang in the room: Visor's
/// hotbar wheel in front of the hand, the movement state icon on the off hand): where it is,
/// as a hand's pose is (metres from the head's rest position, the play space's turns), how wide
/// it is (its image is the menu panel's 16:9; it is as tall as that makes it), and what it shows
/// (the game draws it into the image `Headset::overlay_image` hands out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlaySpec {
    pub pos: Vec3,
    pub rot: Quat,
    pub width: f32,
    /// How tall it is: the image's 16:9 unless the overlay is a strip.
    pub height: f32,
}

impl OverlaySpec {
    /// A panel of the menu image's 16:9 shape, `width` metres wide.
    pub fn new(pos: Vec3, rot: Quat, width: f32) -> Self {
        Self {
            pos,
            rot,
            width,
            height: width * 9.0 / 16.0,
        }
    }
}

/// Where the TV of the headset launcher's room hangs (stage space, metres): the menu panel of a
/// title frame is put there instead of where the title stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TvPanel {
    pub centre: Vec3,
    pub rot: Quat,
    pub width: f32,
}

/// The tracking data as the runtime gave it, in its floor-level room space (metres, +Y up, the
/// device faces -Z at identity): Vivecraft's trackers work in this space.
#[derive(Clone, Copy, Debug)]
pub struct RoomSnapshot {
    /// The head's pose.
    pub head: (Vec3, Quat),
    /// The controllers' aim and grip poses, physical left and right.
    pub aim: [Option<(Vec3, Quat)>; 2],
    pub grip: [Option<(Vec3, Quat)>; 2],
    /// The head's rest position: the game's camera is here.
    pub rest: Vec3,
    /// Milliseconds on the session's clock.
    pub time_ms: u64,
    /// Changes on recenter/reference-space discontinuities.
    pub pose_epoch: u64,
}

/// One controller's buttons at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandButtons {
    /// 0..1.
    pub trigger: f32,
    /// The grip (squeeze), 0..1.
    pub grip: f32,
    /// The lower face button: A on the right controller, X on the left.
    pub primary: bool,
    /// The upper face button: B on the right, Y on the left.
    pub secondary: bool,
    pub stick_click: bool,
}

/// The joints of one tracked hand, in `XR_EXT_hand_tracking` order (palm, wrist, then the thumb,
/// index, middle, ring and little finger from the metacarpal to the tip), in room space.
pub const HAND_JOINTS: usize = 26;
/// The body trackers Visor sends, in the order of its tracker bit mask: waist, chest, left and
/// right foot, ankle and knee, left and right wrist, elbow and shoulder.
pub const BODY_TRACKERS: usize = 14;
/// A joint or tracker pose in room space.
pub type Joint = (Vec3, Quat);

/// What the headset's hand and body tracking gave.
#[derive(Clone, Copy, Debug)]
struct Tracking {
    /// The game wants the hand joints / the body trackers (the XR layer runs them only then).
    wanted: [bool; 2],
    /// The headset can: the extension is there and the system supports it.
    supported: [bool; 2],
    /// Joints of the physical left and right hand, `None` when untracked.
    hands: [Option<[Joint; HAND_JOINTS]>; 2],
    body: Option<[Option<Joint>; BODY_TRACKERS]>,
}

#[derive(Clone, Debug)]
struct State {
    active: bool,
    /// How close the eyes are to a solid block, 0..1 (Visor's `VRCameraOverlaps.blockProximity`).
    in_block: f32,
    tracking: Tracking,
    /// The controllers as a pad (sticks, buttons, triggers).
    pad: PadState,
    /// The head's yaw and pitch in Minecraft's degrees (yaw 0 faces south,
    /// pitch positive looks down), relative to the play space.
    head: Option<[f32; 2]>,
    /// Where the pointer ray meets the menu panel, 0..1 from its top left.
    pointer: Option<[f32; 2]>,
    /// The pointer's left and right buttons (the triggers while a menu shows).
    pointer_buttons: [bool; 2],
    /// A screen or the title is open: the headset shows the menu panel.
    menu: bool,
    /// The game's yaw minus the head's: where the play space faces in the world.
    body_yaw: f32,
    /// Snap turns asked for and not yet taken, in degrees (positive: right).
    snap: f32,
    /// The controllers, physical left and right.
    hands: [Option<HandPose>; 2],
    /// Their buttons, physical left and right.
    buttons: [HandButtons; 2],
    /// How far (metres) the main hand's pointer runs to the menu panel it meets.
    pointer_distance: Option<f32>,
    /// The hotbar slot (0..8, 9 the off hand's) the main hand touches on the HUD
    /// (`InteractiveHotbarModule`).
    hud_slot: Option<usize>,
    overlays: [Option<OverlaySpec>; OVERLAYS],
    /// The handles under the screen's panel (`VROverlay.isCursorOnDragHandle`) and what a hand
    /// does with them now.
    screen_handles: ScreenHandles,
    /// A screen the player resized: its width in metres, kept in the settings by the game.
    screen_scale: Option<f32>,
    /// The kind of screen the menu panel shows, for where it opens.
    menu_kind: MenuKind,
    /// The runtime ended the session (the system menu's Quit): the game
    /// saves and leaves.
    quit: bool,
    /// Y (or Menu) was pressed in play: Visor's game menu opens (`ActionMenu`).
    game_menu: bool,
    keyboard_toggle: bool,
    off_use: bool,
    /// The off-hand controller points the menu's cursor (the hand that clicked last).
    pointer_off: bool,
    /// Movement is by teleporting: the move stick aims the arc and does not walk.
    stick_teleports: bool,
    /// The move stick as the controller gave it (the pad has it zeroed while teleporting).
    move_stick: [f32; 2],
    /// Where the next menu panel opens (play space, from the rest point):
    /// over the block whose screen it shows.
    panel_anchor: Option<Vec3>,
    /// The room's TV: the title's panel hangs there while it is set.
    tv: Option<TvPanel>,
    /// The game takes the main and off triggers for a gesture (a hand in a
    /// button, a bow being drawn): they neither attack nor use.
    triggers_captured: [bool; 2],
    /// Two-handed play (`VRServerSettings.isTwoHandedVR`): the off trigger is a left click too.
    two_handed: bool,
    /// The direction the player aims along (Minecraft yaw and pitch, play
    /// space): the main hand's controller standing, the head seated.
    aim: Option<[f32; 2]>,
    /// The VR settings the game holds (`vivecraft-client-config.json`); `None` before it
    /// hands them over.
    settings: Option<Arc<VrSettings>>,
    /// Haptic pulses asked for and not yet played: (strength 0..1, seconds)
    /// for the physical left and right controllers.
    haptics: [Option<(f32, f32)>; 2],
    /// The head's position from its rest position (the game's camera), in
    /// the play space.
    head_offset: Option<Vec3>,
    /// `VRPlayer.worldScale`: how many world metres a room metre is.
    world_scale: f32,
    /// The options screen asked for the room origin to go back under the player.
    reset_origin: bool,
    /// The options screen asked for the player's height to be calibrated.
    calibrate_height: bool,
    /// The tracking data of the last frame, as the runtime gave it.
    room: Option<RoomSnapshot>,
    /// Buttons the game's own trackers hold down this loop (sneak, jump, backpack), merged
    /// into the pad.
    tracker_pad: PadState,
    /// How far the game moved the player for the head's walk in the room
    /// (play space) since the headset last took it: the rest position follows.
    walked: Vec3,
}

static STATE: Mutex<State> = Mutex::new(State {
    active: false,
    in_block: 0.0,
    tracking: Tracking {
        wanted: [false; 2],
        supported: [false; 2],
        hands: [None; 2],
        body: None,
    },
    pad: PadState {
        buttons: 0,
        left: [0.0; 2],
        right: [0.0; 2],
        triggers: [0.0; 2],
    },
    head: None,
    pointer: None,
    pointer_buttons: [false; 2],
    menu: true,
    body_yaw: 0.0,
    snap: 0.0,
    hands: [None; 2],
    buttons: [HandButtons {
        trigger: 0.0,
        grip: 0.0,
        primary: false,
        secondary: false,
        stick_click: false,
    }; 2],
    pointer_distance: None,
    hud_slot: None,
    overlays: [None; OVERLAYS],
    screen_handles: ScreenHandles {
        visible: false,
        dragging: false,
        resizing: false,
    },
    screen_scale: None,
    menu_kind: MenuKind::Normal,
    quit: false,
    game_menu: false,
    keyboard_toggle: false,
    off_use: false,
    pointer_off: false,
    stick_teleports: false,
    move_stick: [0.0; 2],
    triggers_captured: [false; 2],
    two_handed: false,
    panel_anchor: None,
    tv: None,
    aim: None,
    settings: None,
    haptics: [None; 2],
    head_offset: None,
    world_scale: 1.0,
    walked: Vec3::ZERO,
    reset_origin: false,
    room: None,
    calibrate_height: false,
    tracker_pad: PadState {
        buttons: 0,
        left: [0.0; 2],
        right: [0.0; 2],
        triggers: [0.0; 2],
    },
});

impl State {
    fn left_handed(&self) -> bool {
        self.settings.as_ref().is_some_and(|v| v.left_handed())
    }
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// How close the eyes are to a solid block (0 free, 1 inside): the vignette closes in on it.
pub fn set_in_block(proximity: f32) {
    with(|s| s.in_block = proximity.clamp(0.0, 1.0));
}
pub fn in_block() -> f32 {
    with(|s| if s.active { s.in_block } else { 0.0 })
}

/// The game asks for the hand joints (`hands`) and the body trackers (`body`); the XR layer runs
/// a tracker only while it is wanted.
pub fn set_tracking_wanted(hands: bool, body: bool) {
    with(|s| s.tracking.wanted = [hands, body]);
}
/// Whether the hand and body tracking are wanted.
pub fn tracking_wanted() -> [bool; 2] {
    with(|s| s.tracking.wanted)
}
/// The XR layer says what the headset can do (`[hands, body]`).
pub fn set_tracking_supported(hands: bool, body: bool) {
    with(|s| s.tracking.supported = [hands, body]);
}
pub fn tracking_supported() -> [bool; 2] {
    with(|s| s.tracking.supported)
}
/// The joints of a hand (physical left 0, right 1), room space; `None` when it is not tracked.
pub fn set_hand_joints(hand: usize, joints: Option<[Joint; HAND_JOINTS]>) {
    with(|s| s.tracking.hands[hand.min(1)] = joints);
}
/// The joints of the main (`main`) or off hand.
pub fn hand_joints(main: bool) -> Option<[Joint; HAND_JOINTS]> {
    with(|s| {
        let i = usize::from(main != s.left_handed());
        s.tracking.hands[i].filter(|_| s.active)
    })
}
/// The body trackers, room space; `None` when the body is not tracked.
pub fn set_body_trackers(trackers: Option<[Option<Joint>; BODY_TRACKERS]>) {
    with(|s| s.tracking.body = trackers);
}
pub fn body_trackers() -> Option<[Option<Joint>; BODY_TRACKERS]> {
    with(|s| s.tracking.body.filter(|_| s.active))
}

/// The headset's session runs: its input counts and the game draws for it.
pub fn set_active(on: bool) {
    with(|s| s.active = on);
}
pub fn active() -> bool {
    with(|s| s.active)
}

/// The controllers' state, as a pad.
pub fn set_pad(pad: PadState) {
    with(|s| s.pad = pad);
}
/// The controllers' state while the headset is active.
pub fn pad() -> Option<PadState> {
    with(|s| s.active.then(|| s.pad.merge(&s.tracker_pad)))
}

/// The buttons the game's trackers hold down (a sneak, a jump), merged into [`pad`].
pub fn set_tracker_pad(pad: PadState) {
    with(|s| s.tracker_pad = pad);
}

/// The tracking data in the runtime's room space.
pub fn set_room(room: Option<RoomSnapshot>) {
    with(|s| s.room = room);
}
pub fn room() -> Option<RoomSnapshot> {
    with(|s| s.room.filter(|_| s.active))
}

/// The head's direction (Minecraft yaw and pitch, degrees); `None` while untracked.
pub fn set_head(head: Option<[f32; 2]>) {
    with(|s| s.head = head);
}
pub fn head() -> Option<[f32; 2]> {
    with(|s| s.head.filter(|_| s.active))
}

/// The overlay panel the game wants shown this frame, or none.
pub fn set_overlay(overlay: Option<OverlaySpec>) {
    set_overlay_at(OVERLAY_HAND, overlay);
}
pub fn overlay() -> Option<OverlaySpec> {
    overlay_at(OVERLAY_HAND)
}
/// The same for any of the [`OVERLAYS`] slots.
pub fn set_overlay_at(slot: usize, overlay: Option<OverlaySpec>) {
    with(|s| s.overlays[slot] = overlay);
}
pub fn overlay_at(slot: usize) -> Option<OverlaySpec> {
    with(|s| s.overlays[slot].filter(|_| s.active))
}

pub fn set_screen_handles(handles: ScreenHandles) {
    with(|s| s.screen_handles = handles);
}
pub fn screen_handles() -> ScreenHandles {
    with(|s| s.screen_handles)
}
/// A resized screen's width in metres (`OverlayOptionsResizing.save`): the game keeps it.
pub fn set_screen_scale(width: f32) {
    with(|s| s.screen_scale = Some(width));
}
pub fn take_screen_scale() -> Option<f32> {
    with(|s| s.screen_scale.take())
}

/// Where the pointer meets the menu panel (0..1, from its top left), if it does.
pub fn set_pointer(at: Option<[f32; 2]>) {
    with(|s| s.pointer = at);
}
pub fn pointer() -> Option<[f32; 2]> {
    with(|s| s.pointer.filter(|_| s.active))
}

/// The pointer's left and right buttons.
pub fn set_pointer_buttons(buttons: [bool; 2]) {
    with(|s| s.pointer_buttons = buttons);
}
pub fn pointer_buttons() -> [bool; 2] {
    with(|s| {
        if s.active {
            s.pointer_buttons
        } else {
            [false; 2]
        }
    })
}

/// The game shows a screen (or the title): the headset draws it on a panel.
pub fn set_menu(open: bool) {
    with(|s| s.menu = open);
}
pub fn menu() -> bool {
    with(|s| s.menu)
}

/// The game's yaw minus the head's, in degrees.
pub fn set_body_yaw(yaw: f32) {
    with(|s| s.body_yaw = yaw);
}
pub fn body_yaw() -> f32 {
    with(|s| s.body_yaw)
}

/// Turns the body by `degrees` (a snap turn of the stick), once.
pub fn snap_turn(degrees: f32) {
    with(|s| s.snap += degrees);
}
/// The snap turns since the last call, in degrees.
pub fn take_snap_turn() -> f32 {
    with(|s| std::mem::take(&mut s.snap))
}

/// The controllers as last tracked (physical left, right), relative to the
/// head's rest position.
pub fn set_hands(hands: [Option<HandPose>; 2]) {
    with(|s| s.hands = hands);
}
pub fn hands() -> [Option<HandPose>; 2] {
    with(|s| if s.active { s.hands } else { [None; 2] })
}
/// The controller of the main hand (the right one, the left with reversed hands).
pub fn main_hand() -> Option<HandPose> {
    with(|s| {
        let i = usize::from(!s.left_handed());
        s.hands[i].filter(|_| s.active)
    })
}
/// The controller of the off hand.
pub fn off_hand() -> Option<HandPose> {
    with(|s| {
        let i = usize::from(s.left_handed());
        s.hands[i].filter(|_| s.active)
    })
}

/// The controllers' buttons (physical left, right).
pub fn set_buttons(buttons: [HandButtons; 2]) {
    with(|s| s.buttons = buttons);
}
/// The buttons of the main and the off hand's controllers.
pub fn hand_buttons() -> [HandButtons; 2] {
    with(|s| {
        if !s.active {
            return [HandButtons::default(); 2];
        }
        let main = usize::from(!s.left_handed());
        [s.buttons[main], s.buttons[1 - main]]
    })
}

/// How far the main hand's pointer runs to the menu panel, while it meets one.
pub fn set_pointer_distance(d: Option<f32>) {
    with(|s| s.pointer_distance = d);
}
pub fn pointer_distance() -> Option<f32> {
    with(|s| s.pointer_distance.filter(|_| s.active))
}

/// Hangs the title's panel on the room's TV (`None`: where the title stands).
pub fn set_room_tv(tv: Option<TvPanel>) {
    with(|s| s.tv = tv);
}
pub fn room_tv() -> Option<TvPanel> {
    with(|s| s.tv)
}

/// Where the next menu panel opens (play space, from the rest point), or
/// ahead of the eyes.
pub fn set_panel_anchor(at: Option<Vec3>) {
    with(|s| s.panel_anchor = at);
}
pub fn panel_anchor() -> Option<Vec3> {
    with(|s| s.panel_anchor)
}

/// The game takes the main and the off hand's triggers for a gesture this
/// frame: the headset stops sending them as attack and use.
pub fn set_triggers_captured(captured: [bool; 2]) {
    with(|s| s.triggers_captured = captured);
}
/// Two-handed play is on: the game sets it each frame, the headset's input reads it.
pub fn set_two_handed(on: bool) {
    with(|s| s.two_handed = on);
}
pub fn two_handed() -> bool {
    with(|s| s.two_handed)
}
pub fn triggers_captured() -> [bool; 2] {
    with(|s| s.triggers_captured)
}

/// Y was pressed in play: the game opens Visor's game menu.
pub fn request_game_menu() {
    with(|s| s.game_menu = true);
}
/// Whether the game menu was asked for since the last call, once.
pub fn take_game_menu() -> bool {
    with(|s| std::mem::take(&mut s.game_menu))
}

/// Movement is by teleporting (Visor's `TELEPORT` mode): the move stick aims the arc instead of
/// walking.
pub fn set_stick_teleports(on: bool) {
    with(|s| s.stick_teleports = on);
}
pub fn stick_teleports() -> bool {
    with(|s| s.stick_teleports)
}
/// The move stick as the controller gave it.
pub fn set_move_stick(stick: [f32; 2]) {
    with(|s| s.move_stick = stick);
}
pub fn move_stick() -> [f32; 2] {
    with(|s| if s.active { s.move_stick } else { [0.0; 2] })
}

/// Which hand points the menu's cursor: the off hand (`true`) or the main one.
pub fn set_pointer_off(off: bool) {
    with(|s| s.pointer_off = off);
}
pub fn pointer_off() -> bool {
    with(|s| s.pointer_off)
}

/// The runtime is closing the app: the game should save and quit.
pub fn request_quit() {
    with(|s| s.quit = true);
}
/// Whether the game should save and quit, once.
pub fn take_quit() -> bool {
    with(|s| std::mem::take(&mut s.quit))
}

/// The hotbar slot the main hand touches on the HUD.
pub fn set_hud_slot(slot: Option<usize>) {
    with(|s| s.hud_slot = slot);
}
pub fn hud_slot() -> Option<usize> {
    with(|s| s.hud_slot.filter(|_| s.active))
}

/// What the menu panel shows, for where it opens (`GuiHandler.onScreenChanged`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    Normal,
    Chat,
}
pub fn set_menu_kind(kind: MenuKind) {
    with(|s| s.menu_kind = kind);
}
pub fn menu_kind() -> MenuKind {
    with(|s| s.menu_kind)
}

/// The direction the player aims along (Minecraft yaw and pitch in the play
/// space); `None` while neither the controller nor the head is tracked.
pub fn set_aim(aim: Option<[f32; 2]>) {
    with(|s| s.aim = aim);
}
pub fn aim() -> Option<[f32; 2]> {
    with(|s| s.aim.filter(|_| s.active))
}

/// The game's VR settings, for the headset's side.
pub fn set_settings(settings: &Arc<VrSettings>) {
    with(|s| {
        if !s
            .settings
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, settings))
        {
            s.settings = Some(Arc::clone(settings));
        }
    });
}
/// The settings the game last handed over, or Horizon's defaults before that.
pub fn settings() -> Arc<VrSettings> {
    with(|s| s.settings.clone()).unwrap_or_else(|| Arc::new(VrSettings::horizon()))
}

/// Asks the controller of the main (`main`) or off hand for a pulse of
/// `strength` (0..1) lasting `seconds`; a stronger pending one stays.
pub fn pulse(main: bool, strength: f32, seconds: f32) {
    with(|s| {
        if !s.active {
            return;
        }
        let i = usize::from(main != s.left_handed());
        let pending = &mut s.haptics[i];
        if pending.is_none_or(|(a, _)| a < strength) {
            *pending = Some((strength.clamp(0.0, 1.0), seconds));
        }
    });
}
/// The pulses asked for since the last call (physical left, right).
pub fn take_pulses() -> [Option<(f32, f32)>; 2] {
    with(|s| std::mem::take(&mut s.haptics))
}

/// The VR options' Calibrate Height button.
pub fn request_calibrate_height() {
    with(|s| s.calibrate_height = true);
}
/// Whether Calibrate Height was pressed since the last call.
pub fn take_calibrate_height() -> bool {
    with(|s| std::mem::take(&mut s.calibrate_height))
}

/// The VR options' Reset Origin button.
pub fn request_reset_origin() {
    with(|s| s.reset_origin = true);
}
/// Whether Reset Origin was pressed since the last call.
pub fn take_reset_origin() -> bool {
    with(|s| std::mem::take(&mut s.reset_origin))
}

/// The head's position from its rest position (play space), while tracked.
/// The game's world scale, which the XR layer scales the eyes' and hands' places by.
pub fn set_world_scale(scale: f32) {
    with(|s| s.world_scale = scale.clamp(0.025, 100.0));
}
pub fn world_scale() -> f32 {
    with(|s| s.world_scale)
}
pub fn set_head_offset(offset: Option<Vec3>) {
    with(|s| s.head_offset = offset);
}
pub fn head_offset() -> Option<Vec3> {
    with(|s| s.head_offset.filter(|_| s.active))
}

/// The game moved the player `by` (play space) after the head's walk in the room.
pub fn walked(by: Vec3) {
    with(|s| s.walked += by);
}
/// The room walk the game made since the last call.
pub fn take_walked() -> Vec3 {
    with(|s| std::mem::take(&mut s.walked))
}

/// A play-space offset (from the head's rest position) as the world's, for
/// a body facing `body_yaw` (Minecraft degrees): the play space's -Z ahead
/// turned onto the direction the game calls that yaw.
pub fn to_world(body_yaw: f32, v: Vec3) -> Vec3 {
    body_rotation(body_yaw) * v
}
/// The world's rotation for a body facing `body_yaw` (Minecraft degrees).
pub fn body_rotation(body_yaw: f32) -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI - body_yaw.to_radians())
}

/// Minecraft's yaw and pitch (degrees) of something turned by `q` in the play
/// space: -Z ahead is yaw 0 of the body, turning left lowers the yaw, looking
/// up is a negative pitch.
pub fn angles(q: Quat) -> [f32; 2] {
    direction_angles(q * Vec3::NEG_Z)
}

/// Minecraft's yaw and pitch (degrees) of a direction in the play space.
pub fn direction_angles(d: Vec3) -> [f32; 2] {
    let d = d.normalize_or(Vec3::NEG_Z);
    let turn_left = (-d.x).atan2(-d.z);
    [
        -turn_left.to_degrees(),
        -d.y.clamp(-1.0, 1.0).asin().to_degrees(),
    ]
}

/// The yaw delta (degrees, -180..180) and pitch that turn a player looking
/// along `(yaw, pitch)` toward the head's direction from a body facing
/// `body_yaw`: the head steers the view.
pub fn turn_toward_head(body_yaw: f32, head: [f32; 2], yaw: f32, pitch: f32) -> (f32, f32) {
    let target = body_yaw + head[0];
    let delta = (target - yaw + 180.0).rem_euclid(360.0) - 180.0;
    (delta, head[1].clamp(-90.0, 90.0) - pitch)
}

/// A gameplay binding asks for the same keyboard as the game's VR menu.
pub fn request_keyboard_toggle() {
    with(|s| s.keyboard_toggle = true);
}
pub fn take_keyboard_toggle() -> bool {
    with(|s| std::mem::take(&mut s.keyboard_toggle))
}
/// The rebound off-hand use input; physical button state remains available for UI and gestures.
pub fn set_off_use(on: bool) {
    with(|s| s.off_use = on);
}
pub fn off_use() -> bool {
    with(|s| s.active && s.off_use)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_follow_minecraft_conventions() {
        let close =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        assert!(close(angles(Quat::IDENTITY), [0.0, 0.0]));
        // Turning left (counter-clockwise seen from above) is a lower yaw.
        let left = Quat::from_rotation_y(30f32.to_radians());
        assert!(close(angles(left), [-30.0, 0.0]), "{:?}", angles(left));
        // Looking up is a negative pitch.
        let up = Quat::from_rotation_x(20f32.to_radians());
        assert!(close(angles(up), [0.0, -20.0]), "{:?}", angles(up));
    }

    #[test]
    fn the_play_space_ahead_is_the_bodys_facing_in_the_world() {
        // Minecraft yaw 0 faces south (+Z), 90 west (-X).
        let ahead = to_world(0.0, Vec3::NEG_Z);
        assert!((ahead - Vec3::Z).length() < 1e-5, "{ahead}");
        let ahead = to_world(90.0, Vec3::NEG_Z);
        assert!((ahead - Vec3::NEG_X).length() < 1e-5, "{ahead}");
    }

    #[test]
    fn the_head_turns_the_player_the_short_way_round() {
        // Body facing 170, head 20 to the right of it: the player looks at 190, i.e. -170.
        let (dy, dp) = turn_toward_head(170.0, [20.0, 30.0], -175.0, 0.0);
        assert!((dy - 5.0).abs() < 1e-4, "{dy}");
        assert!((dp - 30.0).abs() < 1e-4);
        // Already there: nothing to turn.
        let (dy, dp) = turn_toward_head(0.0, [45.0, -10.0], 45.0, -10.0);
        assert!(dy.abs() < 1e-4 && dp.abs() < 1e-4);
    }
}
