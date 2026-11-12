//! A headset frame split into its parts, behind a toggle (`horizonVrProfile` in VR Settings >
//! Rendering, or `HORIZON_VR_PROFILE=log|overlay`), for finding which part keeps a frame from
//! the headset's rate.
//!
//! A frame's CPU time is split into the game's simulation (`update`, `tick`), building the
//! frame's geometry (`prepare`), the three OpenXR calls the runtime paces it with (`xr wait`,
//! `xr begin`, `xr acquire`), each eye's recording and submission (`eye L`, `eye R`, with the
//! share that was `submit` and the share spent waiting on the GPU or the submit thread,
//! `stall`), the panel's HUD, the images' layout pass and `xrEndFrame`. The GPU's own time
//! comes from timestamp queries, read a few frames late (`gpu`, per eye pass), and the slowest
//! passes of Horizon's own timer.
//!
//! Off, every call is one relaxed atomic load. On, a line goes to the log every
//! [`REPORT_SECONDS`] and, in the overlay mode, the F3 screen shows the same figures.
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::Instant;

/// Seconds of frames each report covers.
pub const REPORT_SECONDS: f64 = 5.0;

/// How much of the breakdown is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// Averages and worst frames to the log.
    Log,
    /// The log, and the F3 screen opened on the headset's HUD with the same figures.
    Overlay,
}

impl Mode {
    /// The setting's value (`OFF`, `LOG`, `OVERLAY`) or the environment's (`1` is `LOG`).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "OFF" | "0" | "" => Some(Self::Off),
            "LOG" | "1" | "ON" | "TRUE" => Some(Self::Log),
            "OVERLAY" | "2" => Some(Self::Overlay),
            _ => None,
        }
    }
}

/// The parts of a frame, in the order they are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// World streaming, uploads and the other work before the ticks.
    Update,
    /// The game's ticks that came due.
    Tick,
    /// Building the frame's geometry and the HUD.
    Prepare,
    /// `xrWaitFrame`: the runtime holding the frame back to its rate.
    XrWait,
    XrBegin,
    /// Acquiring and waiting for the swapchain images.
    XrAcquire,
    /// The left eye's pass recorded and submitted.
    EyeLeft,
    EyeRight,
    /// Of the eye's time, the queue submission (or its hand-off to the submit thread).
    SubmitLeft,
    SubmitRight,
    /// Of the eye's time, waits for the GPU, the submit thread or the previous frame.
    StallLeft,
    StallRight,
    /// The HUD panel's pass.
    Hud,
    /// The pass that leaves the images in the layout the runtime takes.
    Finish,
    XrEnd,
}

const STAGES: usize = 15;

const NAMES: [&str; STAGES] = [
    "update",
    "tick",
    "prepare",
    "xr wait",
    "xr begin",
    "xr acquire",
    "eye L / stereo",
    "eye R",
    "submit L",
    "submit R",
    "stall L",
    "stall R",
    "hud",
    "finish",
    "xr end",
];

static MODE: AtomicU8 = AtomicU8::new(0);

/// The mode now.
pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Log,
        2 => Mode::Overlay,
        _ => Mode::Off,
    }
}

/// Whether frames are being measured.
#[inline]
pub fn enabled() -> bool {
    MODE.load(Ordering::Relaxed) != 0
}

/// Sets the mode (the setting, read again every so often by [`refresh`]).
pub fn set_mode(mode: Mode) {
    MODE.store(
        match mode {
            Mode::Off => 0,
            Mode::Log => 1,
            Mode::Overlay => 2,
        },
        Ordering::Relaxed,
    );
}

/// Reads `HORIZON_VR_PROFILE`, else the setting `horizonVrProfile`.
pub fn refresh() {
    let wanted = std::env::var("HORIZON_VR_PROFILE")
        .ok()
        .and_then(|v| Mode::parse(&v))
        .or_else(|| {
            let settings = horizon_client_input::headset::settings();
            Mode::parse(settings.enum_name("horizonVrProfile"))
        })
        .unwrap_or(Mode::Off);
    if wanted != mode() {
        log::info!("Horizon XR: frame profile {wanted:?}");
        set_mode(wanted);
    }
}

/// [`refresh`] about once a second at 90 frames a second: the setting may change in the menu,
/// and reading it every frame buys nothing.
pub fn refresh_often() {
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if CALLS.fetch_add(1, Ordering::Relaxed).is_multiple_of(90) {
        refresh();
    }
}

/// One report window's totals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// This frame's parts so far, milliseconds.
    current: [f32; STAGES],
    sums: [f64; STAGES],
    worst: [f32; STAGES],
    frames: u32,
    render_modes: [u32; 2],
    /// The slowest whole frame (its parts summed), and the sum of them all.
    worst_total: f32,
    total_sum: f64,
    active_sum: f64,
    worst_active: f32,
    gpu_sum: f64,
    gpu_worst: f32,
    gpu_count: u32,
    /// Milliseconds on the caller's clock when the window opened.
    opened: Option<f64>,
    /// The slowest passes of Horizon's own GPU timer, with their medians.
    passes: Vec<(String, f32)>,
    /// What an eye pass drew (`Scene`), summed over the passes counted in `scene_count`.
    scene_sum: [f64; 4],
    scene_worst: [u32; 4],
    scene_count: u32,
}

/// What an eye pass drew: the chunk sections, the far terrain tiles (LOD), the terrain draw
/// calls and the indices they draw.
pub const SCENE: [&str; 4] = ["chunks", "lod tiles", "draw calls", "indices"];

/// A finished window.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub frames: u32,
    pub render_modes: [u32; 2],
    pub fps: f32,
    /// Average milliseconds per frame by part.
    pub average: [f32; STAGES],
    pub worst: [f32; STAGES],
    pub average_total: f32,
    pub worst_total: f32,
    /// Wall-clock app spans excluding XR calls and nested eye stalls, not hardware utilization.
    pub average_active: f32,
    pub worst_active: f32,
    /// Average and worst GPU time of an eye's pass, with the timed passes' count.
    pub gpu: Option<(f32, f32, u32)>,
    pub passes: Vec<(String, f32)>,
    /// Average and worst of what an eye pass drew, in `SCENE`'s order.
    pub scene: Option<([f32; 4], [u32; 4])>,
}

impl Profile {
    /// Adds `ms` to this frame's `stage`.
    pub fn record(&mut self, stage: Stage, ms: f32) {
        self.current[stage as usize] += ms;
    }

    /// One timed eye pass, from the timestamp queries.
    pub fn gpu(&mut self, ms: f32) {
        self.gpu_sum += f64::from(ms);
        self.gpu_worst = self.gpu_worst.max(ms);
        self.gpu_count += 1;
    }

    /// What an eye pass drew.
    pub fn scene(&mut self, drew: [u32; 4]) {
        for ((sum, worst), v) in self
            .scene_sum
            .iter_mut()
            .zip(&mut self.scene_worst)
            .zip(drew)
        {
            *sum += f64::from(v);
            *worst = (*worst).max(v);
        }
        self.scene_count += 1;
    }

    /// Horizon's own timed passes, medians in milliseconds; the slowest few are kept.
    pub fn set_passes(&mut self, mut passes: Vec<(String, f32)>) {
        passes.sort_by(|a, b| b.1.total_cmp(&a.1));
        passes.truncate(8);
        self.passes = passes;
    }

    /// Drops what the frame in progress recorded (a frame that drew no world).
    pub fn discard(&mut self) {
        self.current = [0.0; STAGES];
    }

    /// The frame is done at `now_ms` (any monotonic clock): closes the window and returns its
    /// report once it has run [`REPORT_SECONDS`].
    pub fn finish(&mut self, now_ms: f64) -> Option<Report> {
        // The eye's submit and stall are inside the eye's time, not on top of it.
        let total: f32 = self
            .current
            .iter()
            .enumerate()
            .filter(|(i, _)| !matches!(i, 8..=11))
            .map(|(_, ms)| ms)
            .sum();
        for ((sum, worst), ms) in self.sums.iter_mut().zip(&mut self.worst).zip(self.current) {
            *sum += f64::from(ms);
            *worst = worst.max(ms);
        }
        let runtime = [
            Stage::XrWait,
            Stage::XrBegin,
            Stage::XrAcquire,
            Stage::XrEnd,
        ]
        .into_iter()
        .map(|s| self.current[s as usize])
        .sum::<f32>();
        let stalls =
            self.current[Stage::StallLeft as usize] + self.current[Stage::StallRight as usize];
        let active = (total - runtime - stalls).max(0.0);
        self.active_sum += f64::from(active);
        self.worst_active = self.worst_active.max(active);
        self.worst_total = self.worst_total.max(total);
        self.total_sum += f64::from(total);
        self.frames += 1;
        self.current = [0.0; STAGES];
        let opened = *self.opened.get_or_insert(now_ms);
        let span = now_ms - opened;
        if span < REPORT_SECONDS * 1000.0 {
            return None;
        }
        let frames = self.frames.max(1) as f32;
        let report = Report {
            frames: self.frames,
            render_modes: self.render_modes,
            fps: self.frames as f32 / (span / 1000.0) as f32,
            average: self.sums.map(|s| (s / f64::from(frames)) as f32),
            worst: self.worst,
            average_total: (self.total_sum / f64::from(frames)) as f32,
            worst_total: self.worst_total,
            average_active: (self.active_sum / f64::from(frames)) as f32,
            worst_active: self.worst_active,
            gpu: (self.gpu_count > 0).then(|| {
                (
                    (self.gpu_sum / f64::from(self.gpu_count)) as f32,
                    self.gpu_worst,
                    self.gpu_count,
                )
            }),
            passes: self.passes.clone(),
            scene: (self.scene_count > 0).then(|| {
                let n = f64::from(self.scene_count);
                (self.scene_sum.map(|s| (s / n) as f32), self.scene_worst)
            }),
        };
        let passes = std::mem::take(&mut self.passes);
        *self = Self {
            opened: Some(now_ms),
            passes,
            ..Self::default()
        };
        Some(report)
    }
}

impl Report {
    /// The figures as text lines, for the log and the F3 screen.
    pub fn lines(&self) -> Vec<String> {
        let part = |values: &[f32; STAGES], i: usize| format!("{} {:.1}", NAMES[i], values[i]);
        let join = |values: &[f32; STAGES], indices: &[usize]| {
            indices
                .iter()
                .map(|&i| part(values, i))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut lines = vec![format!(
            "{} frames, {:.1} fps, paced spans {:.1} ms (worst {:.1})",
            self.frames, self.fps, self.average_total, self.worst_total
        )];
        lines.push(format!("app CPU spans excluding XR calls/eye waits: {:.1} ms (worst {:.1}); not CPU utilization",
            self.average_active, self.worst_active));
        lines.push(format!(
            "world frames: {} separate-eye, {} multiview; GPU samples arrive late",
            self.render_modes[0], self.render_modes[1]
        ));
        for (label, values) in [("avg", &self.average), ("max", &self.worst)] {
            lines.push(format!(
                "{label}: sim [{}] prep [{}] xr [{}]",
                join(values, &[0, 1]),
                join(values, &[2]),
                join(values, &[3, 4, 5]),
            ));
            lines.push(format!(
                "{label}: eyes [{}] (submit {:.1}+{:.1}, stall {:.1}+{:.1}) [{}]",
                join(values, &[6, 7]),
                values[8],
                values[9],
                values[10],
                values[11],
                join(values, &[12, 13, 14]),
            ));
        }
        match self.gpu {
            Some((average, worst, n)) => {
                lines.push(format!(
                    "gpu per world render (one eye or stereo): {average:.1} ms (worst {worst:.1}, {n} timed)"
                ));
            }
            None => lines.push("gpu per world render (one eye or stereo): not timed".to_owned()),
        }
        if let Some((average, worst)) = &self.scene {
            lines.push(format!(
                "per world render: {}",
                SCENE
                    .iter()
                    .enumerate()
                    .map(|(i, name)| format!("{name} {:.0} (max {})", average[i], worst[i]))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.passes.is_empty() {
            lines.push(format!(
                "gpu passes: {}",
                self.passes
                    .iter()
                    .map(|(name, ms)| format!("{name} {ms:.2}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines
    }
}

struct Global {
    profile: Profile,
    started: Instant,
    last: Vec<String>,
}

static STATE: Mutex<Option<Global>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Global) -> R) -> R {
    let mut slot = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(slot.get_or_insert_with(|| Global {
        profile: Profile::default(),
        started: Instant::now(),
        last: Vec::new(),
    }))
}

/// Adds a part of the frame in progress (nothing while off).
#[inline]
pub fn record(stage: Stage, elapsed: std::time::Duration) {
    if enabled() {
        let ms = elapsed.as_secs_f32() * 1000.0;
        with(|g| g.profile.record(stage, ms));
    }
}

/// A GPU time of an eye pass, in milliseconds (nothing while off).
#[inline]
pub fn gpu(ms: f32) {
    if enabled() {
        with(|g| g.profile.gpu(ms));
    }
}

/// Which world path produced this frame, so fallback use is visible in profiles.
pub fn render_mode(stereo: bool) {
    if enabled() {
        with(|g| g.profile.render_modes[usize::from(stereo)] += 1);
    }
}

/// Horizon's own passes' GPU medians (nothing while off).
pub fn passes(passes: Vec<(String, f32)>) {
    if enabled() {
        with(|g| g.profile.set_passes(passes));
    }
}

/// What an eye pass drew (nothing while off).
pub fn scene(drew: [u32; 4]) {
    if enabled() {
        with(|g| g.profile.scene(drew));
    }
}

/// The frame drew a world: closes it, and logs the window when it is full.
pub fn frame_done() {
    if !enabled() {
        return;
    }
    with(|g| {
        let now = g.started.elapsed().as_secs_f64() * 1000.0;
        if let Some(report) = g.profile.finish(now) {
            let lines = report.lines();
            log::info!("Horizon XR: frame profile: {}", lines.join(" | "));
            if crate::mesher::merging() {
                // Summed over every section meshed so far (remeshed ones count again).
                log::info!(
                    "Horizon XR: merged block faces: {:?}",
                    crate::mesher::merge_stats()
                );
            }
            g.last = lines;
        }
    });
}

/// The frame drew no world: what it recorded is dropped.
pub fn frame_skipped() {
    if enabled() {
        with(|g| g.profile.discard());
    }
}

/// The last window's lines, for the F3 screen (empty until one is full).
pub fn overlay_lines() -> Vec<String> {
    if enabled() {
        with(|g| g.last.clone())
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two frames: 2 ms update, 3 ms tick (then 5 and 1), eyes 4 and 2 ms of which submit 1 and 0.5.
    fn feed(profile: &mut Profile, update: f32, tick: f32, left: f32, right: f32) {
        profile.record(Stage::Update, update);
        profile.record(Stage::Tick, tick);
        profile.record(Stage::EyeLeft, left);
        profile.record(Stage::EyeRight, right);
        profile.record(Stage::SubmitLeft, 1.0);
        profile.record(Stage::SubmitRight, 0.5);
    }

    #[test]
    fn runtime_pacing_is_not_reported_as_cpu_work() {
        let mut p = Profile::default();
        for at in [0.0, 5000.0] {
            p.record(Stage::Prepare, 1.0);
            p.record(Stage::EyeLeft, 1.0);
            p.record(Stage::StallLeft, 0.5);
            p.record(Stage::XrWait, 9.0);
            if let Some(report) = p.finish(at) {
                assert_eq!(report.average_total, 11.0);
                assert_eq!(report.average_active, 1.5);
                assert_eq!(report.worst_active, 1.5);
                assert!(report.lines()[0].contains("paced spans 11.0"));
                assert!(report.lines()[1].contains("1.5 ms"));
            }
        }
    }

    #[test]
    fn a_window_reports_averages_and_the_worst_frame() {
        let mut p = Profile::default();
        feed(&mut p, 2.0, 3.0, 4.0, 2.0);
        assert!(p.finish(0.0).is_none());
        feed(&mut p, 4.0, 1.0, 6.0, 2.0);
        // Not yet 5 s since the window opened at 0 ms.
        assert!(p.finish(4_000.0).is_none());
        p.gpu(3.0);
        p.gpu(5.0);
        feed(&mut p, 0.0, 0.0, 0.0, 0.0);
        let report = p.finish(5_000.0).expect("5 s have passed");
        assert_eq!(report.frames, 3);
        // Update (2 + 4 + 0) / 3, eye L (4 + 6 + 0) / 3.
        assert!((report.average[Stage::Update as usize] - 2.0).abs() < 1e-5);
        assert!((report.average[Stage::EyeLeft as usize] - 10.0 / 3.0).abs() < 1e-5);
        assert_eq!(report.worst[Stage::EyeLeft as usize], 6.0);
        // A frame's total is its parts without the submit and stall shares inside the eyes:
        // frame one 2 + 3 + 4 + 2 = 11, frame two 4 + 1 + 6 + 2 = 13, frame three 0.
        assert_eq!(report.worst_total, 13.0);
        assert!((report.average_total - 8.0).abs() < 1e-5);
        assert!((report.fps - 0.6).abs() < 1e-5);
        assert_eq!(report.gpu, Some((4.0, 5.0, 2)));
    }

    #[test]
    fn the_next_window_starts_empty() {
        let mut p = Profile::default();
        feed(&mut p, 1.0, 1.0, 1.0, 1.0);
        p.finish(0.0);
        feed(&mut p, 1.0, 1.0, 1.0, 1.0);
        assert!(p.finish(6_000.0).is_some());
        feed(&mut p, 9.0, 0.0, 0.0, 0.0);
        assert!(p.finish(7_000.0).is_none());
        feed(&mut p, 9.0, 0.0, 0.0, 0.0);
        let second = p
            .finish(11_000.0)
            .expect("5 s after the window opened at 6 s");
        assert_eq!(second.frames, 2);
        assert_eq!(second.average[Stage::Update as usize], 9.0);
        assert_eq!(second.gpu, None);
    }

    #[test]
    fn a_discarded_frame_leaves_no_trace() {
        let mut p = Profile::default();
        feed(&mut p, 50.0, 50.0, 50.0, 50.0);
        p.discard();
        feed(&mut p, 1.0, 0.0, 0.0, 0.0);
        p.finish(0.0);
        let report = p.finish(5_000.0).unwrap();
        assert_eq!(report.worst[Stage::Update as usize], 1.0);
    }

    #[test]
    fn the_lines_name_every_part_and_the_slowest_passes() {
        let mut p = Profile::default();
        feed(&mut p, 2.0, 3.0, 4.0, 2.0);
        p.record(Stage::XrWait, 7.0);
        p.set_passes(vec![
            ("Sky".into(), 0.2),
            ("World".into(), 3.5),
            ("Entities".into(), 1.0),
        ]);
        p.finish(0.0);
        let lines = p.finish(5_000.0).unwrap().lines();
        assert!(lines[0].starts_with("2 frames"), "{lines:?}");
        let average = lines
            .iter()
            .find(|line| line.starts_with("avg: sim"))
            .unwrap();
        assert!(
            average.contains("avg: sim [update 1.0, tick 1.5]"),
            "{lines:?}"
        );
        assert!(average.contains("xr wait 3.5"), "{lines:?}");
        assert!(
            lines
                .last()
                .unwrap()
                .starts_with("gpu passes: World 3.50, Entities 1.00")
        );
    }

    #[test]
    fn the_scene_is_averaged_over_the_eye_passes() {
        let mut p = Profile::default();
        p.scene([100, 10, 300, 40_000]);
        p.scene([140, 12, 340, 60_000]);
        p.finish(0.0);
        let report = p.finish(5_000.0).unwrap();
        assert_eq!(
            report.scene,
            Some(([120.0, 11.0, 320.0, 50_000.0], [140, 12, 340, 60_000]))
        );
        let text = report.lines().join("|");
        assert!(
            text.contains("chunks 120 (max 140), lod tiles 11 (max 12)"),
            "{text}"
        );
    }

    #[test]
    fn modes_parse_the_setting_and_the_environment_forms() {
        assert_eq!(Mode::parse("OVERLAY"), Some(Mode::Overlay));
        assert_eq!(Mode::parse("1"), Some(Mode::Log));
        assert_eq!(Mode::parse("off"), Some(Mode::Off));
        assert_eq!(Mode::parse("maybe"), None);
    }
}
