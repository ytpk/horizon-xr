//! The headset launcher's room as an application state (`App::Room`, docs/quest-launcher-plan.md,
//! phase 6): a window and a renderer like the loading screen's, a room drawn behind the panel
//! (`gfx::room_layout`), the panel hung on the room's TV with the launcher's page on it
//! (`quest_room`), and the pointer's trigger on its buttons. Play gives the window and the
//! renderer to `Loading` as they are: the same process, the same OpenXR session, the same device.
use super::*;
use glam::{Quat, Vec3};
use horizon_client::gfx::room_layout;
use horizon_client::quest_room::{Event, RoomUi};
use horizon_client::room::{Handoff, Page, RoomAction, RoomHost, RoomModel};

pub(super) struct Room {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    boot: horizon_client::hud::HudAtlas,
    pub(super) settings: Settings,
    options: Option<Options>,
    host: Box<dyn RoomHost>,
    ui: RoomUi,
    started: Instant,
    /// The real block textures read from the game's files, once they exist.
    textures: Vec<(usize, Vec<u8>)>,
    textures_from: Option<PathBuf>,
    frames: u64,
    /// The room's language is chosen (the first model says what the headset speaks).
    language_set: bool,
    /// The pointer's first frame was logged.
    pointer_logged: bool,
    /// The ray meets the TV (for the log).
    pointer_on: bool,
    pub(super) error: Option<Box<dyn std::error::Error>>,
}

/// What the room wants of the event loop.
pub(super) enum Next {
    Waiting,
    Play(Handoff),
    Quit,
}

/// Starts the game as the room: the loader waits for Play.
pub(super) fn run(options: Options, host: Box<dyn RoomHost>, started: Instant) -> Result<()> {
    let settings = Settings::load(&Game::options_path());
    horizon_client::hud::GUI_SCALE.store(settings.gui_scale, Ordering::Relaxed);
    log::info!("Horizon: the headset launcher's room starts");
    let room = Room {
        window: None,
        renderer: None,
        boot: horizon_client::hud::HudAtlas::boot(),
        settings,
        options: Some(options),
        host,
        ui: RoomUi::default(),
        started,
        textures: Vec::new(),
        textures_from: None,
        frames: 0,
        language_set: false,
        pointer_logged: false,
        pointer_on: false,
        error: None,
    };
    run_app(App::Room(Box::new(room)))
}

impl Room {
    pub(super) fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(options) = &self.options else { return };
        let size = options.window_size.map(f64::from);
        let attributes = Window::default_attributes()
            .with_title("Horizon")
            .with_window_icon(horizon_client::window::icon())
            .with_inner_size(winit::dpi::LogicalSize::new(size[0], size[1]))
            .with_visible(!options.offscreen);
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(e.into());
                event_loop.exit();
                return;
            }
        };
        horizon_client::telemetry::note_display(&window);
        match loading_renderer(
            window.clone(),
            options.offscreen,
            Some(&self.boot),
            None,
            None,
            self.settings.horizon.api,
        ) {
            Ok(r) => self.renderer = Some(r),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    pub(super) fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    pub(super) fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) -> Next {
        if self.error.is_some() || horizon_threads::stop_requests() > 0 {
            return Next::Quit;
        }
        // The headset's system menu quit the app.
        if horizon_client::headset::take_quit() {
            return Next::Quit;
        }
        if let Some(window) = &self.window {
            if self.options.as_ref().is_some_and(|o| o.offscreen) {
                self.draw();
            } else {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        match self.host.take_play() {
            Some(handoff) => Next::Play(handoff),
            None => Next::Waiting,
        }
    }

    /// The real block textures of the game's files, read when they first appear.
    fn real_textures(&mut self, model: &RoomModel) {
        if model.resources == self.textures_from {
            return;
        }
        self.textures_from = model.resources.clone();
        self.textures = model
            .resources
            .as_deref()
            .map(room_layout::load_real)
            .unwrap_or_default();
        if !self.textures.is_empty() {
            log::info!(
                "Horizon: the room turns real: {} block textures from the game's files",
                self.textures.len()
            );
        }
    }

    /// The words on the TV: the game's own language setting if the player made one, else the
    /// headset's language, else English (every string is bundled, none needs the game's files).
    fn choose_language(&mut self, model: &RoomModel) {
        if self.language_set {
            return;
        }
        self.language_set = true;
        let chosen = self.settings.file.string("lang");
        let code = if !chosen.is_empty() && chosen != "en_us" {
            chosen
        } else {
            model
                .system_language
                .clone()
                .unwrap_or_else(|| "en_us".to_owned())
        };
        log::info!("Horizon: the room speaks {code}");
        horizon_client::lang::set(horizon_client::lang::Language::load(&[], &code));
    }

    fn draw(&mut self) {
        let model = self.host.model();
        self.choose_language(&model);
        self.real_textures(&model);
        // The TV the panel hangs on, and the room behind it.
        horizon_client::headset::set_menu(true);
        horizon_client::headset::set_menu_kind(horizon_client::headset::MenuKind::Normal);
        horizon_client::headset::set_room_tv(Some(horizon_client::headset::TvPanel {
            centre: room_layout::TV_CENTRE,
            rot: room_layout::TV_ROT,
            width: room_layout::TV_WIDTH,
        }));
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        renderer.set_room(true);
        if !self.textures.is_empty() {
            renderer.ensure_room_textures(&self.textures);
        }
        // What the game's menus draw besides the panel: the ray of the hand that points, to
        // where it meets the TV, and both controllers as grey boxes (`headset_pointer_lines`,
        // `headset_menu_hand`). In the room's space, with the eyes and the TV.
        let tracked = horizon_client::headset::room().filter(|_| horizon_client::headset::active());
        let mesh = tracked.map_or_else(Vec::new, |room| {
            let main = usize::from(!horizon_client::headset::settings().left_handed());
            pointer_mesh(
                room.aim,
                main,
                horizon_client::headset::pointer_off(),
                horizon_client::headset::pointer_distance(),
            )
        });
        if !mesh.is_empty() && !self.pointer_logged {
            self.pointer_logged = true;
            log::info!(
                "Horizon room: the pointer is drawn ({} vertices, ray {:?} m)",
                mesh.len(),
                horizon_client::headset::pointer_distance()
            );
        }
        renderer.set_room_pointer(&mesh);
        let mut batch = HudBatch::new([renderer.config.width, renderer.config.height]);
        let pointer = horizon_client::headset::pointer();
        if pointer.is_some() != self.pointer_on {
            self.pointer_on = pointer.is_some();
            match pointer {
                Some([u, v]) => log::info!(
                    "Horizon room: the ray meets the TV at ({u:.2}, {v:.2}), {:?} m from the hand",
                    horizon_client::headset::pointer_distance()
                ),
                None => log::info!("Horizon room: the ray left the TV"),
            }
        }
        let down = horizon_client::headset::pointer_buttons()[0];
        self.ui.time = self.started.elapsed().as_secs_f32();
        let (event, moved) = self.ui.frame(&self.boot, &mut batch, &model, pointer, down);
        if moved {
            // A tick in the hand that points.
            horizon_client::headset::pulse(!horizon_client::headset::pointer_off(), 0.12, 0.02);
        }
        if let Some(Event::Action(action)) = event {
            horizon_client::headset::pulse(!horizon_client::headset::pointer_off(), 0.4, 0.04);
            log::info!("Horizon room: pressed {action:?}");
            self.host.act(action);
        }
        if let Some(Event::RenderScale(delta) | Event::RefreshRate(delta)) = event {
            use horizon_client_input::headset_display;
            let key = if matches!(event, Some(Event::RenderScale(_))) {
                "renderScaleFactor"
            } else {
                "horizonRefreshRate"
            };
            let current = self.settings.vr.float(key);
            let next = if key == "renderScaleFactor" {
                headset_display::step_scale(current, delta)
            } else {
                headset_display::snapshot().map_or(current, |i| {
                    headset_display::step_rate(current, delta, &i.rates)
                })
            };
            if next != current {
                Arc::make_mut(&mut self.settings.vr).set_float(key, next);
                self.settings.save(&Game::options_path());
                horizon_client::headset::set_settings(&self.settings.vr);
                horizon_client::headset::pulse(!horizon_client::headset::pointer_off(), 0.4, 0.04);
            }
        }
        if matches!(event, Some(Event::Multiview)) {
            let enabled = !self.settings.vr.bool("horizonMultiview");
            Arc::make_mut(&mut self.settings.vr).set_bool("horizonMultiview", enabled);
            self.settings.save(&Game::options_path());
            horizon_client::headset::set_settings(&self.settings.vr);
            horizon_client::headset::pulse(!horizon_client::headset::pointer_off(), 0.4, 0.04);
        }
        if matches!(
            event,
            Some(
                Event::DynamicResolution
                    | Event::Foveation
                    | Event::SuperResolution
                    | Event::OpaqueWater
                    | Event::FoveatedMultiview
            )
        ) {
            let settings = Arc::make_mut(&mut self.settings.vr);
            match event {
                Some(Event::DynamicResolution) => {
                    settings.set_bool(
                        "horizonDynamicResolution",
                        !settings.bool("horizonDynamicResolution"),
                    );
                }
                Some(Event::FoveatedMultiview) => {
                    settings.set_bool(
                        "horizonFoveatedMultiview",
                        !settings.bool("horizonFoveatedMultiview"),
                    );
                }
                Some(Event::OpaqueWater) => {
                    settings.set_bool("horizonOpaqueWater", !settings.bool("horizonOpaqueWater"));
                }
                Some(Event::SuperResolution) => {
                    settings.set_bool(
                        "horizonSuperResolution",
                        !settings.bool("horizonSuperResolution"),
                    );
                }
                Some(Event::Foveation) => {
                    let next = match settings.enum_name("horizonFoveation") {
                        "OFF" => "LOW",
                        "LOW" => "MEDIUM",
                        "MEDIUM" => "HIGH",
                        _ => "OFF",
                    };
                    settings.set("horizonFoveation", next);
                }
                _ => unreachable!(),
            }
            self.settings.save(&Game::options_path());
            horizon_client::headset::set_settings(&self.settings.vr);
            horizon_client::headset::pulse(!horizon_client::headset::pointer_off(), 0.4, 0.04);
        }
        // A developer's press with no hand: `HORIZON_ROOM_PRESS=signin@120` (also retry, play,
        // signout, cancel) presses that button once at that frame, for a headset on a desk.
        if let Some((name, at)) = std::env::var("HORIZON_ROOM_PRESS").ok().and_then(|v| {
            v.split_once('@')
                .map(|(a, b)| (a.to_owned(), b.parse::<u64>().ok()))
        }) && at == Some(self.frames)
        {
            let action = match name.as_str() {
                "signin" => Some(RoomAction::SignIn),
                "retry" => Some(RoomAction::Retry),
                "play" => Some(RoomAction::Play),
                "signout" => Some(RoomAction::SignOut),
                "cancel" => Some(RoomAction::CancelSignIn),
                _ => None,
            };
            if let Some(action) = action {
                log::info!("Horizon room: dev press {action:?}");
                self.host.act(action);
            }
        }
        self.frames += 1;
        let frame = loading_frame(&batch);
        match renderer.render(&frame) {
            Ok(true) => {}
            Ok(false) => {
                // The frame was not shown (the first one often is not): its glyphs never reached
                // the atlas, so they are drawn again.
                horizon_client::ui_theme::reset_atlas();
            }
            Err(e) => log::warn!("Horizon: the room's frame failed: {e}"),
        }
    }

    /// Play: the window and the renderer go on to the loading screen, the account's session to
    /// the game, the loader starts on the prepared files.
    pub(super) fn start_loading(&mut self, handoff: Handoff) -> Result<Loading> {
        let mut options = self.options.take().ok_or("the room already handed over")?;
        let window = self.window.take().ok_or("the room has no window")?;
        let renderer = self.renderer.take().ok_or("the room has no renderer")?;
        log::info!(
            "Horizon room: Play on {} (session: {})",
            handoff.resources.display(),
            handoff.session.is_some()
        );
        options.resources = handoff.resources.clone();
        if let Some(session) = &handoff.session {
            // The signed-in account, in memory: no session file, the token never touches the disk.
            let json = serde_json::json!({
                "name": session.name,
                "uuid": session.uuid,
                "access_token": session.token,
                "user_type": "msa",
                "xuid": session.xuid,
            });
            match horizon_client::auth::AuthSession::parse(&json.to_string()) {
                Ok(parsed) => {
                    options.player_name = Some(parsed.name.clone());
                    horizon_client::auth::set_current(parsed);
                    horizon_client::auth::prepare_chat_keys(true);
                }
                Err(e) => log::warn!("Horizon: not signed in: {e}"),
            }
        }
        let settings = before_loading(&options, &handoff.resources)?;
        let mut loading = build_loading(
            options,
            settings,
            handoff.resources,
            None,
            Instant::now(),
            Some((window, renderer)),
        )?;
        loading.room_textures = std::mem::take(&mut self.textures);
        Ok(loading)
    }
}

/// How wide the ray is: Visor's cursor line, 3.2 mm.
const RAY_WIDTH: f32 = 0.0032;
/// ARGB of Visor's menu ray, `CURSOR_DEFAULT_COLOR` (228, 228, 228), as 0xRRGGBB.
const RAY_COLOUR: u32 = 0xE4_E4E4;
/// How far ahead of the controller the ray starts, and how far it goes where it meets no TV
/// (the game's menus' `LASER_GAP`, `MENU_LASER_MISS`).
const RAY_GAP: f32 = 0.04;
const RAY_MISS: f32 = 0.6;
/// The grey of a controller's box (`VRArmHelper.renderMenuHand`, lit at the full 15).
const CONTROLLER_GREY: u32 = 0x40_4040;

/// A thin box from `a` to `b`, `width` across, as triangles (36 vertices) of colour `rgb`.
fn prism(a: Vec3, b: Vec3, width: f32, rgb: u32, out: &mut Vec<([f32; 3], u32)>) {
    let along = (b - a).normalize_or_zero();
    if along == Vec3::ZERO {
        return;
    }
    let helper = if along.y.abs() > 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let right = along.cross(helper).normalize() * (width / 2.0);
    let up = along.cross(right).normalize() * (width / 2.0);
    let corner = |end: Vec3, sx: f32, sy: f32| (end + right * sx + up * sy).to_array();
    let ring = |end: Vec3| {
        [
            corner(end, -1.0, -1.0),
            corner(end, 1.0, -1.0),
            corner(end, 1.0, 1.0),
            corner(end, -1.0, 1.0),
        ]
    };
    let (ra, rb) = (ring(a), ring(b));
    let mut quad = |q: [[f32; 3]; 4]| {
        for i in [0, 1, 2, 0, 2, 3] {
            out.push((q[i], rgb));
        }
    };
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad([ra[i], ra[j], rb[j], rb[i]]);
    }
    quad([ra[3], ra[2], ra[1], ra[0]]);
    quad([rb[0], rb[1], rb[2], rb[3]]);
}

/// The room's pointer in the room's space (the TV's and the eyes'): the ray of the hand that
/// points, from just ahead of the controller to where it meets the TV (`distance` from the
/// controller), and both controllers as the grey boxes the game's menus draw. `aim` are the
/// controllers' aim poses, physical left and right; `main` the physical index of the main
/// hand; `off` whether the off hand points.
fn pointer_mesh(
    aim: [Option<(Vec3, Quat)>; 2],
    main: usize,
    off: bool,
    distance: Option<f32>,
) -> Vec<([f32; 3], u32)> {
    let mut out = Vec::new();
    for pose in aim.into_iter().flatten() {
        let mut quads = Vec::new();
        let at = glam::Mat4::from_rotation_translation(pose.1, pose.0);
        super::headset::menu_hand_vertices(at, [0.0, 0.0], 15, &mut quads);
        for q in quads.as_chunks::<4>().0 {
            for i in [0, 1, 2, 0, 2, 3] {
                out.push((q[i].pos, CONTROLLER_GREY));
            }
        }
    }
    let pointing = if off { 1 - main } else { main };
    if let Some((origin, q)) = aim[pointing] {
        let along = q * Vec3::NEG_Z;
        let length = distance.unwrap_or(RAY_MISS);
        prism(
            origin + along * RAY_GAP,
            origin + along * length.max(RAY_GAP),
            RAY_WIDTH,
            RAY_COLOUR,
            &mut out,
        );
    }
    out
}

impl Loading {
    /// While the game loads from the room, the TV goes on in the room with the loading screen.
    pub(super) fn room_frame(&mut self) {
        if !self.room {
            return;
        }
        horizon_client::headset::set_menu(true);
        horizon_client::headset::set_room_tv(Some(horizon_client::headset::TvPanel {
            centre: room_layout::TV_CENTRE,
            rot: room_layout::TV_ROT,
            width: room_layout::TV_WIDTH,
        }));
        if let Some(renderer) = &mut self.renderer {
            renderer.set_room(true);
            if !self.room_textures.is_empty() {
                renderer.ensure_room_textures(&self.room_textures);
            }
        }
    }
}

// ----- The developer's fixed room ---------------------------------------------------------------

/// `HORIZON_ROOM_STATE=idle|signin|checking|download|sounds|prepare|error|noaccount|offline` on a
/// computer with `HORIZON_SIM_HEADSET=1`: the room with that page and no launcher behind it
/// (docs/quest-launcher-plan.md). `HORIZON_ROOM_REAL=<resources>` turns the room real with that
/// folder's block textures, `HORIZON_ROOM_AUTOPLAY=<frames>` presses Play after that many frames,
/// to try the hand-off to the game.
pub(super) fn dev_host() -> Option<Box<dyn RoomHost>> {
    let state = std::env::var("HORIZON_ROOM_STATE").ok()?;
    let resources = std::env::var_os("HORIZON_ROOM_REAL").map(PathBuf::from);
    let autoplay = std::env::var("HORIZON_ROOM_AUTOPLAY")
        .ok()
        .and_then(|v| v.parse().ok());
    Some(Box::new(DevHost::new(&state, resources, autoplay)))
}

struct DevHost {
    model: RoomModel,
    resources: Option<PathBuf>,
    autoplay: Option<u64>,
    calls: u64,
    play: bool,
}

impl DevHost {
    fn new(state: &str, resources: Option<PathBuf>, autoplay: Option<u64>) -> Self {
        use horizon_client::room::{Code, Failure, News, Progress};
        let mut model = RoomModel {
            version: "26.3".into(),
            resources: resources.clone(),
            ..RoomModel::default()
        };
        let account = Some("Player".to_owned());
        match state {
            "signin" => {
                model.page = Page::SignIn;
                model.code = Some(Code {
                    user_code: "L7KQ4XJ9".into(),
                    url: "https://www.microsoft.com/link".into(),
                    qr_url: "https://www.microsoft.com/link?otc=L7KQ4XJ9".into(),
                    expires_in: 874,
                });
            }
            "checking" => {
                model.page = Page::Checking;
                model.step = Some("minecraft".into());
                model.account = account;
            }
            "download" | "sounds" => {
                model.page = Page::Download;
                model.sounds_stage = state == "sounds";
                model.account = account;
                model.download = Some(Progress {
                    files_done: 1432,
                    files_total: 5214,
                    bytes_done: 61 << 20,
                    bytes_total: 197 << 20,
                    bytes_per_sec: 2.9e6,
                    eta_secs: Some(47),
                });
            }
            "prepare" => {
                model.page = Page::Prepare;
                model.account = account;
                model.prepare = Some((
                    "Exporting client data (VanillaEntityModelExporter)".into(),
                    0.42,
                ));
            }
            "error" => {
                model.page = Page::Download;
                model.account = account;
                model.error = Some(Failure {
                    kind: "network".into(),
                    message: "https://piston-data.mojang.com/v1/objects/0a1b2c3d timed out".into(),
                });
            }
            "noaccount" => {}
            _ => {
                model.page = Page::Idle;
                model.account = account;
                model.offline = state == "offline";
                model.can_play = true;
                model.system_language = Some("ru_ru".into());
                model.news = Some(News {
                    title: "Nightly build".into(),
                    date: "2026-11-07".into(),
                    lines: vec![
                        "The headset launcher: sign in, download and play from a room on the headset".into(),
                        "Visor's keyboard and menu room are in the game".into(),
                    ],
                });
            }
        }
        Self {
            model,
            resources,
            autoplay,
            calls: 0,
            play: false,
        }
    }
}

impl RoomHost for DevHost {
    fn model(&mut self) -> RoomModel {
        self.calls += 1;
        if self.autoplay.is_some_and(|n| self.calls >= n) {
            self.play = true;
        }
        self.model.clone()
    }

    fn act(&mut self, action: RoomAction) {
        match action {
            RoomAction::Play => self.play = true,
            RoomAction::SignIn => {
                *self = Self::new("signin", self.resources.clone(), self.autoplay)
            }
            RoomAction::CancelSignIn | RoomAction::SignOut => {
                *self = Self::new("noaccount", self.resources.clone(), self.autoplay)
            }
            RoomAction::Retry => *self = Self::new("idle", self.resources.clone(), self.autoplay),
            RoomAction::SetSounds(on) => self.model.sounds = on,
            RoomAction::SetSystemLanguage(on) => self.model.use_system_language = on,
            RoomAction::CancelDownload => {}
        }
    }

    fn take_play(&mut self) -> Option<Handoff> {
        self.play.then(|| {
            self.play = false;
            Handoff {
                resources: self.resources.clone().unwrap_or_else(|| {
                    PathBuf::from(
                        std::env::var_os("HORIZON_ROOM_RESOURCES")
                            .unwrap_or_else(|| ".local/horizon-client/26.3".into()),
                    )
                }),
                session: None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extent(mesh: &[([f32; 3], u32)], colour: u32, axis: usize) -> (f32, f32) {
        let values = mesh.iter().filter(|v| v.1 == colour).map(|v| v.0[axis]);
        (
            values.clone().fold(f32::MAX, f32::min),
            values.fold(f32::MIN, f32::max),
        )
    }

    #[test]
    fn the_ray_runs_from_just_ahead_of_the_controller_to_the_tv_and_the_controllers_are_grey_boxes()
    {
        // Both controllers 0.4 m in front of the stage's middle at 1.4 m, aiming straight at the
        // TV (-Z), which stands 2.86 m away: 2.46 m from the controller.
        let right = (Vec3::new(0.2, 1.4, -0.4), Quat::IDENTITY);
        let left = (Vec3::new(-0.2, 1.4, -0.4), Quat::IDENTITY);
        let mesh = pointer_mesh([Some(left), Some(right)], 1, false, Some(2.46));
        // Two boxes of 36 vertices and the ray's 36.
        assert_eq!(mesh.len(), 3 * 36);
        let (near, far) = extent(&mesh, RAY_COLOUR, 2);
        assert!(
            (near - -2.86).abs() < 1e-4 && (far - (-0.4 - 0.04)).abs() < 1e-4,
            "{near} {far}"
        );
        // It is the right (main) hand's: centred on x = 0.2, 3.2 mm across.
        let (low, high) = extent(&mesh, RAY_COLOUR, 0);
        assert!(((low + high) / 2.0 - 0.2).abs() < 1e-5 && ((high - low) - RAY_WIDTH).abs() < 1e-5);
        // The controllers: 18 cm back from each (the box runs +Z from the controller).
        let (a, b) = extent(&mesh, CONTROLLER_GREY, 2);
        assert!(
            (a - -0.4).abs() < 1e-5 && (b - (-0.4 + 0.18)).abs() < 1e-5,
            "{a} {b}"
        );
    }

    #[test]
    fn the_hand_that_clicked_last_points_and_a_ray_that_meets_nothing_is_short() {
        let right = (Vec3::new(0.2, 1.4, -0.4), Quat::IDENTITY);
        let left = (Vec3::new(-0.2, 1.4, -0.4), Quat::IDENTITY);
        // The off hand (the left one, with the main hand right) has the cursor.
        let mesh = pointer_mesh([Some(left), Some(right)], 1, true, None);
        let (low, high) = extent(&mesh, RAY_COLOUR, 0);
        assert!(((low + high) / 2.0 - -0.2).abs() < 1e-5);
        // No TV in the way: 0.6 m from the controller, the game's menus' miss length.
        let (near, _) = extent(&mesh, RAY_COLOUR, 2);
        assert!((near - (-0.4 - 0.6)).abs() < 1e-4, "{near}");
        // Left-handed (the main hand is the left): the same cursor flag points the other hand.
        let mesh = pointer_mesh([Some(left), Some(right)], 0, false, None);
        let (low, high) = extent(&mesh, RAY_COLOUR, 0);
        assert!(((low + high) / 2.0 - -0.2).abs() < 1e-5);
    }

    #[test]
    fn an_untracked_controller_has_no_box_and_no_hands_no_ray() {
        let right = (Vec3::new(0.2, 1.4, -0.4), Quat::IDENTITY);
        // Only the right is tracked; the left (off) hand has the cursor: nothing to point from.
        let mesh = pointer_mesh([None, Some(right)], 1, true, Some(2.0));
        assert_eq!(mesh.len(), 36);
        assert!(mesh.iter().all(|v| v.1 == CONTROLLER_GREY));
        assert!(pointer_mesh([None, None], 1, false, Some(2.0)).is_empty());
    }
}
