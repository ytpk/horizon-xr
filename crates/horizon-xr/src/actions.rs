// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: assets/vivecraft/input/oculus_defaults.json (Touch bindings). Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! The controllers through OpenXR's actions, bound for Meta Quest Touch
//! (`/interaction_profiles/oculus/touch_controller`, which Quest 2, 3, 3S and
//! Pro controllers all offer).
use glam::{Quat, Vec3};
use openxr as xr;

/// A tracked pose in the play space.
pub type Pose = (Vec3, Quat);

/// What the controllers did at one moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub move_stick: [f32; 2],
    pub turn_stick: [f32; 2],
    /// Left, right.
    pub triggers: [f32; 2],
    pub grips: [f32; 2],
    pub stick_clicks: [bool; 2],
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub menu: bool,
    /// Each hand's pointing ray (`aim/pose`: -Z along the ray), left and right.
    pub aim: [Option<Pose>; 2],
    /// Each hand's grip (`grip/pose`), left and right.
    pub grip: [Option<Pose>; 2],
}

pub struct Controls {
    set: xr::ActionSet,
    hands: [xr::Path; 2],
    move_stick: xr::Action<xr::Vector2f>,
    turn_stick: xr::Action<xr::Vector2f>,
    trigger: xr::Action<f32>,
    grip: xr::Action<f32>,
    stick_click: xr::Action<bool>,
    a: xr::Action<bool>,
    b: xr::Action<bool>,
    x: xr::Action<bool>,
    y: xr::Action<bool>,
    menu: xr::Action<bool>,
    aim: xr::Action<xr::Posef>,
    grip_pose: xr::Action<xr::Posef>,
    haptic: xr::Action<xr::Haptic>,
    /// The hands' aim and grip spaces, left and right.
    aim_spaces: [xr::Space; 2],
    grip_spaces: [xr::Space; 2],
}

fn err(what: &'static str) -> impl Fn(xr::sys::Result) -> String {
    move |e| format!("controller {what}: {e}")
}

impl Controls {
    pub fn new(instance: &xr::Instance, session: &xr::Session<xr::Vulkan>) -> Result<Self, String> {
        let path = |p: &str| instance.string_to_path(p).map_err(err("path"));
        let hands = [path("/user/hand/left")?, path("/user/hand/right")?];
        let set = instance
            .create_action_set("gameplay", "Gameplay", 0)
            .map_err(err("action set"))?;
        let both = &hands[..];
        let move_stick = set
            .create_action::<xr::Vector2f>("move", "Move", &[])
            .map_err(err("move"))?;
        let turn_stick = set
            .create_action::<xr::Vector2f>("turn", "Turn", &[])
            .map_err(err("turn"))?;
        let trigger = set
            .create_action::<f32>("trigger", "Attack / use", both)
            .map_err(err("trigger"))?;
        let grip = set
            .create_action::<f32>("grip", "Hotbar", both)
            .map_err(err("grip"))?;
        let stick_click = set
            .create_action::<bool>("stick_click", "Sprint / sneak", both)
            .map_err(err("stick click"))?;
        let a = set
            .create_action::<bool>("jump", "Jump", &[])
            .map_err(err("a"))?;
        let b = set
            .create_action::<bool>("drop", "Drop / back", &[])
            .map_err(err("b"))?;
        let x = set
            .create_action::<bool>("swap_hands", "Swap hands", &[])
            .map_err(err("x"))?;
        let y = set
            .create_action::<bool>("inventory", "Inventory", &[])
            .map_err(err("y"))?;
        let menu = set
            .create_action::<bool>("pause", "Pause", &[])
            .map_err(err("menu"))?;
        let aim = set
            .create_action::<xr::Posef>("aim", "Point", both)
            .map_err(err("aim"))?;
        let grip_pose = set
            .create_action::<xr::Posef>("hand", "Hand", both)
            .map_err(err("grip pose"))?;
        let haptic = set
            .create_action::<xr::Haptic>("buzz", "Buzz", both)
            .map_err(err("haptic"))?;
        let profile = path("/interaction_profiles/oculus/touch_controller")?;
        let bindings = [
            xr::Binding::new(&move_stick, path("/user/hand/left/input/thumbstick")?),
            xr::Binding::new(&turn_stick, path("/user/hand/right/input/thumbstick")?),
            xr::Binding::new(&trigger, path("/user/hand/left/input/trigger/value")?),
            xr::Binding::new(&trigger, path("/user/hand/right/input/trigger/value")?),
            xr::Binding::new(&grip, path("/user/hand/left/input/squeeze/value")?),
            xr::Binding::new(&grip, path("/user/hand/right/input/squeeze/value")?),
            xr::Binding::new(
                &stick_click,
                path("/user/hand/left/input/thumbstick/click")?,
            ),
            xr::Binding::new(
                &stick_click,
                path("/user/hand/right/input/thumbstick/click")?,
            ),
            xr::Binding::new(&a, path("/user/hand/right/input/a/click")?),
            xr::Binding::new(&b, path("/user/hand/right/input/b/click")?),
            xr::Binding::new(&x, path("/user/hand/left/input/x/click")?),
            xr::Binding::new(&y, path("/user/hand/left/input/y/click")?),
            xr::Binding::new(&menu, path("/user/hand/left/input/menu/click")?),
            xr::Binding::new(&aim, path("/user/hand/left/input/aim/pose")?),
            xr::Binding::new(&aim, path("/user/hand/right/input/aim/pose")?),
            xr::Binding::new(&grip_pose, path("/user/hand/left/input/grip/pose")?),
            xr::Binding::new(&grip_pose, path("/user/hand/right/input/grip/pose")?),
            xr::Binding::new(&haptic, path("/user/hand/left/output/haptic")?),
            xr::Binding::new(&haptic, path("/user/hand/right/output/haptic")?),
        ];
        instance
            .suggest_interaction_profile_bindings(profile, &bindings)
            .map_err(err("bindings"))?;
        session
            .attach_action_sets(&[&set])
            .map_err(err("attaching"))?;
        let space = |action: &xr::Action<xr::Posef>, hand: xr::Path| {
            action
                .create_space(session, hand, xr::Posef::IDENTITY)
                .map_err(err("hand space"))
        };
        let aim_spaces = [space(&aim, hands[0])?, space(&aim, hands[1])?];
        let grip_spaces = [space(&grip_pose, hands[0])?, space(&grip_pose, hands[1])?];
        Ok(Self {
            set,
            hands,
            move_stick,
            turn_stick,
            trigger,
            grip,
            stick_click,
            a,
            b,
            x,
            y,
            menu,
            aim,
            grip_pose,
            haptic,
            aim_spaces,
            grip_spaces,
        })
    }

    /// The controllers at `time` (syncs the actions first).
    pub fn read(
        &self,
        session: &xr::Session<xr::Vulkan>,
        base: &xr::Space,
        time: xr::Time,
    ) -> State {
        if let Err(e) = session.sync_actions(&[xr::ActiveActionSet::new(&self.set)]) {
            log::warn!("Horizon XR: syncing the controllers: {e}");
            return State::default();
        }
        let none = xr::Path::NULL;
        let stick = |action: &xr::Action<xr::Vector2f>| {
            action
                .state(session, none)
                .ok()
                .filter(|s| s.is_active)
                .map_or([0.0; 2], |s| [s.current_state.x, s.current_state.y])
        };
        let float = |action: &xr::Action<f32>, hand: xr::Path| {
            action
                .state(session, hand)
                .ok()
                .filter(|s| s.is_active)
                .map_or(0.0, |s| s.current_state)
        };
        let held = |action: &xr::Action<bool>, hand: xr::Path| {
            action
                .state(session, hand)
                .ok()
                .is_some_and(|s| s.is_active && s.current_state)
        };
        let [left, right] = self.hands;
        let locate = |action: &xr::Action<xr::Posef>, space: &xr::Space, hand: xr::Path| {
            action
                .is_active(session, hand)
                .unwrap_or(false)
                .then(|| space.locate(base, time).ok())
                .flatten()
                .filter(|l| {
                    l.location_flags.contains(
                        xr::SpaceLocationFlags::POSITION_VALID
                            | xr::SpaceLocationFlags::ORIENTATION_VALID,
                    )
                })
                .map(|l| {
                    (
                        super::vec3(l.pose.position),
                        super::quat(l.pose.orientation),
                    )
                })
        };
        let aim = [
            locate(&self.aim, &self.aim_spaces[0], left),
            locate(&self.aim, &self.aim_spaces[1], right),
        ];
        let grip = [
            locate(&self.grip_pose, &self.grip_spaces[0], left),
            locate(&self.grip_pose, &self.grip_spaces[1], right),
        ];
        State {
            move_stick: stick(&self.move_stick),
            turn_stick: stick(&self.turn_stick),
            triggers: [float(&self.trigger, left), float(&self.trigger, right)],
            grips: [float(&self.grip, left), float(&self.grip, right)],
            stick_clicks: [
                held(&self.stick_click, left),
                held(&self.stick_click, right),
            ],
            a: held(&self.a, none),
            b: held(&self.b, none),
            x: held(&self.x, none),
            y: held(&self.y, none),
            menu: held(&self.menu, none),
            aim,
            grip,
        }
    }
}

impl Controls {
    /// Buzzes the left (0) or right (1) controller at `strength` (0..1) for `seconds`.
    pub fn buzz(
        &self,
        session: &xr::Session<xr::Vulkan>,
        hand: usize,
        strength: f32,
        seconds: f32,
    ) {
        let event = xr::HapticVibration::new()
            .amplitude(strength.clamp(0.0, 1.0))
            .duration(xr::Duration::from_nanos((seconds.max(0.0) * 1e9) as i64))
            .frequency(xr::FREQUENCY_UNSPECIFIED);
        if let Err(e) = self
            .haptic
            .apply_feedback(session, self.hands[hand], &event)
        {
            log::warn!("Horizon XR: haptics: {e}");
        }
    }
}
