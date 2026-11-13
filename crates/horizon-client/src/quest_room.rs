//! The pages of the headset launcher's TV (docs/quest-launcher-plan.md, phase 5): drawn with
//! Horizon's design system (`ui_theme`) and the bundled fonts only, so that they stand before any
//! Mojang file exists, and hit-tested by the very layout that draws them. Every visible word is a
//! `horizon.quest.room.*` language key.
//!
//! One call per frame, [`RoomUi::frame`]: the model (what the launcher says), the pointer on the
//! panel (0..1) and whether the trigger is down; it draws the page and answers the press that
//! just ended on a button.
use crate::hud::{HudAtlas, HudBatch};
use crate::lang;
use crate::no_resources::wrap;
use crate::ui_theme::{Face, Ui, color};
use horizon_client_input::room::{Page, RoomAction, RoomModel};

/// Every language key the room draws (a test reads each in `en_us` and `ru_ru`).
pub const KEYS: &[&str] = &[
    "sign_in",
    "auth.live_provider",
    "no_account.title",
    "no_account.body",
    "no_account.eula",
    "code.title",
    "code.step_open",
    "code.step_enter",
    "code.scan",
    "code.expires",
    "code.waiting",
    "cancel",
    "connecting",
    "check.title",
    "step.microsoft",
    "step.xbox",
    "step.minecraft",
    "step.ownership",
    "step.profile",
    "download.title",
    "download.title_sounds",
    "download.files",
    "download.size",
    "download.speed",
    "download.eta",
    "download.skip",
    "download.note",
    "prepare.title",
    "prepare.title_sounds",
    "prepare.note",
    "error.title",
    "error.network",
    "error.not_owner",
    "error.no_profile",
    "error.not_approved",
    "error.not_configured",
    "error.xbox_no_profile",
    "error.xbox_region",
    "error.xbox_adult",
    "error.xbox_child",
    "error.code_expired",
    "error.declined",
    "error.sign_in_expired",
    "error.no_space",
    "error.checksum",
    "error.cancelled",
    "error.jvm_failed",
    "error.other",
    "retry",
    "details",
    "sign_out",
    "idle.signed_in",
    "idle.not_signed_in",
    "idle.version",
    "idle.play",
    "idle.settings",
    "idle.checking",
    "idle.offline",
    "idle.news",
    "idle.no_news",
    "settings.title",
    "settings.sounds",
    "settings.language",
    "settings.language_none",
    "settings.on",
    "settings.off",
    "settings.back",
    "settings.graphics",
    "graphics.title",
    "graphics.resolution",
    "graphics.scale",
    "graphics.per_eye",
    "graphics.refresh",
    "graphics.display",
    "graphics.apply",
    "graphics.override",
    "graphics.extras",
    "graphics.dynamic.on",
    "graphics.dynamic.off",
    "graphics.mqsr.on",
    "graphics.mqsr.off",
    "graphics.mqsr.unavailable",
    "graphics.ffr.off",
    "graphics.ffr.low",
    "graphics.ffr.medium",
    "graphics.ffr.high",
    "graphics.dynamic.note",
    "graphics.mqsr.note",
    "graphics.ffr.note",
    "graphics.foveated_multiview.on",
    "graphics.foveated_multiview.off",
    "graphics.unknown",
    "graphics.spacewarp",
    "graphics.spacewarp.active",
    "graphics.spacewarp.native",
    "graphics.spacewarp.unavailable",
    "graphics.multiview.on",
    "graphics.water.on",
    "graphics.water.off",
    "graphics.multiview.off",
];

fn t(key: &str) -> String {
    lang::get(&format!("horizon.quest.room.{key}"))
}

fn tf(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    lang::tr(&format!("horizon.quest.room.{key}"), args)
}

/// What a press on the TV did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Action(RoomAction),
    /// Settings opened or closed (the room's own page, no launcher involved).
    Settings(bool),
    Graphics(bool),
    RenderScale(i8),
    RefreshRate(i8),
    Multiview,
    GraphicsExtras(bool),
    DynamicResolution,
    Foveation,
    FoveatedMultiview,
    SuperResolution,
    OpaqueWater,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    rect: [f32; 4],
    event: Event,
}

impl Hit {
    fn contains(&self, (x, y): (f32, f32)) -> bool {
        let [rx, ry, w, h] = self.rect;
        x >= rx && x < rx + w && y >= ry && y < ry + h
    }
}

/// The page's state between frames.
#[derive(Default)]
pub struct RoomUi {
    qr: Option<(String, Vec<Vec<bool>>)>,
    settings: bool,
    graphics: bool,
    graphics_extras: bool,
    hits: Vec<Hit>,
    /// The button the trigger went down on.
    pressed: Option<Event>,
    was_down: bool,
    /// The button under the pointer, for a haptic tick when it changes.
    hover: Option<Event>,
    /// Seconds, for the little animations (set by the caller, 0 when still).
    pub time: f32,
}

const MARGIN: f32 = 56.0;
const H1: f32 = 46.0;
const BODY: f32 = 24.0;
const BODY_LINE: f32 = 34.0;
const SMALL: f32 = 20.0;
const SMALL_LINE: f32 = 28.0;
const BUTTON_TEXT: f32 = 26.0;
/// The cursor dot's radius in dp.
const CURSOR_RADIUS: f32 = 6.0;
fn danger() -> u32 {
    color::hex(horizon_client::menu_style::get().settings.danger, 255)
}
fn good() -> u32 {
    color::hex(horizon_client::menu_style::get().settings.success, 255)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Primary,
    Secondary,
    /// A button that cannot be pressed now.
    Off,
}

/// A QR code's modules, `true` dark, with no quiet zone.
pub fn qr_modules(text: &str) -> Option<Vec<Vec<bool>>> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    Some(
        (0..n)
            .map(|y| (0..n).map(|x| qr.get_module(x, y)).collect())
            .collect(),
    )
}

/// `seconds` as `m:ss`, or `h:mm:ss`.
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1 << 20) as f64;
    if mb < 10.0 {
        format!("{mb:.1}")
    } else {
        format!("{mb:.0}")
    }
}

/// The page's title and what the headline of an error is.
fn error_text(kind: &str) -> String {
    const KINDS: &[&str] = &[
        "network",
        "not_owner",
        "no_profile",
        "not_approved",
        "not_configured",
        "xbox_no_profile",
        "xbox_region",
        "xbox_adult",
        "xbox_child",
        "code_expired",
        "declined",
        "sign_in_expired",
        "no_space",
        "checksum",
        "cancelled",
        "jvm_failed",
    ];
    t(&format!(
        "error.{}",
        if KINDS.contains(&kind) { kind } else { "other" }
    ))
}

struct Canvas<'a, 'b> {
    ui: &'a mut Ui<'b>,
    hits: &'a mut Vec<Hit>,
    cursor: Option<(f32, f32)>,
    pressed: Option<Event>,
}

impl Canvas<'_, '_> {
    fn text(&mut self, face: Face, size: f32, line: f32, x: f32, y: f32, ink: u32, s: &str) -> f32 {
        self.ui.text_line(face, size, line, x, y, ink, s)
    }

    fn centred(&mut self, face: Face, size: f32, cx: f32, y: f32, ink: u32, s: &str) {
        let w = self.ui.measure(face, size, s);
        self.text(face, size, size * 1.4, cx - w / 2.0, y, ink, s);
    }

    /// Wrapped text; the y after the last line.
    fn paragraph(
        &mut self,
        face: Face,
        size: f32,
        line: f32,
        x: f32,
        mut y: f32,
        width: f32,
        ink: u32,
        s: &str,
    ) -> f32 {
        for l in wrap(self.ui, face, size, s, width) {
            self.text(face, size, line, x, y, ink, &l);
            y += line;
        }
        y
    }

    /// A pill button; it can be pressed unless `Look::Off`.
    fn button(&mut self, event: Event, label: &str, [x, y, w, h]: [f32; 4], look: Look, size: f32) {
        let over = self.cursor.is_some_and(|c| {
            c.0 >= x && c.0 < x + w && c.1 >= y && c.1 < y + h && look != Look::Off
        });
        let held = over && self.pressed == Some(event);
        let (plate, ink) = match (look, over, held) {
            (Look::Off, ..) => (color::raised(), color::muted()),
            (Look::Primary, _, true) => {
                (color::with_alpha(color::accent(), 0.7), color::on_accent())
            }
            (Look::Primary, true, _) => {
                (color::with_alpha(color::accent(), 0.88), color::on_accent())
            }
            (Look::Primary, ..) => (color::accent(), color::on_accent()),
            (Look::Secondary, _, true) => (color::border(), color::text()),
            (Look::Secondary, true, _) => (color::hover(), color::text()),
            (Look::Secondary, ..) => (color::raised(), color::text()),
        };
        self.ui.rounded(x, y, w, h, h / 2.0, plate);
        if over && look == Look::Secondary {
            self.ui.border(x, y, w, h, h / 2.0, color::border());
        }
        let lw = self.ui.measure(Face::Medium, size, label);
        self.text(
            Face::Medium,
            size,
            h,
            x + ((w - lw) / 2.0).round(),
            y,
            ink,
            label,
        );
        if look != Look::Off {
            self.hits.push(Hit {
                rect: [x, y, w, h],
                event,
            });
        }
    }

    /// A button as wide as its label needs.
    fn fit(&mut self, event: Event, label: &str, x: f32, y: f32, h: f32, look: Look) -> f32 {
        let w = (self.ui.measure(Face::Medium, BUTTON_TEXT, label) + 64.0).round();
        self.button(event, label, [x, y, w, h], look, BUTTON_TEXT);
        w
    }

    fn header(&mut self, model: &RoomModel) {
        let w = self.ui.w;
        self.text(
            Face::SemiBold,
            34.0,
            48.0,
            MARGIN,
            22.0,
            color::text(),
            "Horizon",
        );
        if let Some(name) = &model.account {
            let nw = self.ui.measure(Face::Medium, BODY, name);
            let x = w - MARGIN - nw;
            self.text(Face::Medium, BODY, 48.0, x, 22.0, color::text(), name);
            if let Some(head) = model.head.as_ref().filter(|h| h.len() == 8 * 8 * 4) {
                let side = 5.0;
                let (hx, hy) = (x - 16.0 - 8.0 * side, 22.0 + (48.0 - 8.0 * side) / 2.0);
                for (i, px) in head.chunks(4).enumerate() {
                    let (cx, cy) = ((i % 8) as f32, (i / 8) as f32);
                    let c = color::hex(
                        u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]),
                        px[3],
                    );
                    self.ui
                        .rounded(hx + cx * side, hy + cy * side, side, side, 0.0, c);
                }
            }
        }
        self.ui
            .rounded(MARGIN, 84.0, w - 2.0 * MARGIN, 1.0, 0.0, color::border());
    }

    // ----- Pages -------------------------------------------------------------------------------

    fn no_account(&mut self, model: &RoomModel) {
        let w = self.ui.w;
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            130.0,
            color::text(),
            &t("no_account.title"),
        );
        let mut y = self.paragraph(
            Face::Regular,
            BODY,
            BODY_LINE,
            MARGIN,
            214.0,
            (w - 2.0 * MARGIN).min(820.0),
            color::muted(),
            &t("no_account.body"),
        );
        if model.prismarine_live {
            y = self.paragraph(
                Face::Regular,
                SMALL,
                SMALL_LINE,
                MARGIN,
                y + 16.0,
                (w - 2.0 * MARGIN).min(820.0),
                color::muted(),
                &t("auth.live_provider"),
            );
        }
        self.fit(
            Event::Action(RoomAction::SignIn),
            &t("sign_in"),
            MARGIN,
            y + 36.0,
            88.0,
            Look::Primary,
        );
        let h = self.ui.h;
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            h - 70.0,
            color::muted(),
            &t("no_account.eula"),
        );
        if model.connecting {
            self.text(
                Face::Regular,
                SMALL,
                SMALL_LINE,
                MARGIN,
                h - 110.0,
                color::muted(),
                &t("connecting"),
            );
        }
    }

    fn sign_in(&mut self, model: &RoomModel, qr: &[Vec<bool>]) {
        let w = self.ui.w;
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            120.0,
            color::text(),
            &t("code.title"),
        );
        if model.prismarine_live {
            self.text(
                Face::Regular,
                SMALL,
                SMALL_LINE,
                MARGIN,
                180.0,
                color::muted(),
                &t("auth.live_provider"),
            );
        }
        let Some(code) = &model.code else {
            self.text(
                Face::Regular,
                BODY,
                BODY_LINE,
                MARGIN,
                220.0,
                color::muted(),
                &t("connecting"),
            );
            self.fit(
                Event::Action(RoomAction::CancelSignIn),
                &t("cancel"),
                MARGIN,
                330.0,
                72.0,
                Look::Secondary,
            );
            return;
        };
        let url = code
            .url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        self.text(
            Face::Regular,
            BODY,
            BODY_LINE,
            MARGIN,
            200.0,
            color::muted(),
            &tf("code.step_open", &[&url]),
        );
        self.text(
            Face::Regular,
            BODY,
            BODY_LINE,
            MARGIN,
            250.0,
            color::muted(),
            &t("code.step_enter"),
        );
        // The code, big, in a plate.
        let code_size = 92.0;
        let cw = self.ui.measure(Face::Mono, code_size, &code.user_code);
        self.ui
            .rounded(MARGIN, 292.0, cw + 56.0, 128.0, 20.0, color::raised());
        self.text(
            Face::Mono,
            code_size,
            128.0,
            MARGIN + 28.0,
            292.0,
            color::accent(),
            &code.user_code,
        );
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            440.0,
            color::muted(),
            &tf("code.expires", &[&clock(code.expires_in)]),
        );
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            474.0,
            color::muted(),
            &t("code.waiting"),
        );
        self.fit(
            Event::Action(RoomAction::CancelSignIn),
            &t("cancel"),
            MARGIN,
            540.0,
            72.0,
            Look::Secondary,
        );
        // The QR code on a light card with a quiet zone of its own.
        let card = 380.0;
        let (cx, cy) = (w - MARGIN - card, 140.0);
        self.ui.rounded(cx, cy, card, card, 24.0, 0xFFFF_FFFF);
        let n = qr.len() as f32;
        if n > 0.0 {
            let module = ((card - 48.0) / n).floor().max(1.0);
            let (ox, oy) = (
                cx + ((card - module * n) / 2.0).round(),
                cy + ((card - module * n) / 2.0).round(),
            );
            for (y, row) in qr.iter().enumerate() {
                let mut x = 0;
                while x < row.len() {
                    if !row[x] {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < row.len() && row[x] {
                        x += 1;
                    }
                    self.ui.rounded(
                        ox + start as f32 * module,
                        oy + y as f32 * module,
                        (x - start) as f32 * module,
                        module,
                        0.0,
                        0xFF00_0000,
                    );
                }
            }
        }
        self.centred(
            Face::Regular,
            SMALL,
            cx + card / 2.0,
            cy + card + 18.0,
            color::muted(),
            &t("code.scan"),
        );
    }

    fn checking(&mut self, model: &RoomModel) {
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            130.0,
            color::text(),
            &t("check.title"),
        );
        let line = match (&model.step, model.connecting) {
            (Some(step), _) => t(&format!("step.{step}")),
            (None, _) => t("connecting"),
        };
        self.text(
            Face::Regular,
            BODY,
            BODY_LINE,
            MARGIN,
            220.0,
            color::muted(),
            &line,
        );
        if model.account.is_some() {
            self.fit(
                Event::Action(RoomAction::SignOut),
                &t("sign_out"),
                MARGIN,
                320.0,
                64.0,
                Look::Secondary,
            );
        }
    }

    fn bar(&mut self, y: f32, fraction: f32) {
        let w = self.ui.w - 2.0 * MARGIN;
        self.ui.rounded(MARGIN, y, w, 28.0, 14.0, color::raised());
        let filled = (w * fraction.clamp(0.0, 1.0)).round();
        if filled >= 28.0 {
            self.ui
                .rounded(MARGIN, y, filled, 28.0, 14.0, color::accent());
        } else if filled > 0.0 {
            self.ui.rounded(
                MARGIN,
                y,
                28.0,
                28.0,
                14.0,
                color::with_alpha(color::accent(), 0.5),
            );
        }
    }

    fn download(&mut self, model: &RoomModel) {
        let title = if model.sounds_stage {
            "download.title_sounds"
        } else {
            "download.title"
        };
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            130.0,
            color::text(),
            &t(title),
        );
        match &model.download {
            Some(p) => {
                let fraction = if p.bytes_total > 0 {
                    p.bytes_done as f32 / p.bytes_total as f32
                } else {
                    0.0
                };
                self.text(
                    Face::Mono,
                    56.0,
                    76.0,
                    MARGIN,
                    200.0,
                    color::accent(),
                    &format!("{}%", (fraction * 100.0).floor() as u32),
                );
                self.bar(300.0, fraction);
                self.text(
                    Face::Regular,
                    BODY,
                    BODY_LINE,
                    MARGIN,
                    346.0,
                    color::text(),
                    &tf("download.files", &[&p.files_done, &p.files_total]),
                );
                let mut line = tf(
                    "download.size",
                    &[&megabytes(p.bytes_done), &megabytes(p.bytes_total)],
                );
                if p.bytes_per_sec >= 1.0 {
                    line.push_str("   ");
                    line.push_str(&tf("download.speed", &[&megabytes(p.bytes_per_sec as u64)]));
                }
                if let Some(eta) = p.eta_secs {
                    line.push_str("   ");
                    line.push_str(&tf("download.eta", &[&clock(eta)]));
                }
                self.text(
                    Face::Regular,
                    BODY,
                    BODY_LINE,
                    MARGIN,
                    382.0,
                    color::muted(),
                    &line,
                );
            }
            None => {
                self.text(
                    Face::Regular,
                    BODY,
                    BODY_LINE,
                    MARGIN,
                    220.0,
                    color::muted(),
                    &t("connecting"),
                );
            }
        }
        let width = self.ui.w - 2.0 * MARGIN;
        self.paragraph(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            450.0,
            width.min(900.0),
            color::muted(),
            &t("download.note"),
        );
        let label = if model.sounds_stage {
            t("download.skip")
        } else {
            t("cancel")
        };
        self.fit(
            Event::Action(RoomAction::CancelDownload),
            &label,
            MARGIN,
            548.0,
            72.0,
            Look::Secondary,
        );
    }

    fn prepare(&mut self, model: &RoomModel) {
        let title = if model.sounds_stage {
            "prepare.title_sounds"
        } else {
            "prepare.title"
        };
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            130.0,
            color::text(),
            &t(title),
        );
        let (line, fraction) = model.prepare.clone().unwrap_or_default();
        self.text(
            Face::Mono,
            56.0,
            76.0,
            MARGIN,
            200.0,
            color::accent(),
            &format!("{}%", (fraction * 100.0).floor() as u32),
        );
        self.bar(300.0, fraction);
        let width = self.ui.w - 2.0 * MARGIN;
        let shown: String = line.chars().take(110).collect();
        self.text(
            Face::Mono,
            SMALL,
            SMALL_LINE,
            MARGIN,
            346.0,
            color::muted(),
            &shown,
        );
        self.paragraph(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            450.0,
            width.min(900.0),
            color::muted(),
            &t("prepare.note"),
        );
    }

    fn error(&mut self, model: &RoomModel) {
        let Some(error) = &model.error else { return };
        let width = (self.ui.w - 2.0 * MARGIN).min(900.0);
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            130.0,
            danger(),
            &t("error.title"),
        );
        let y = self.paragraph(
            Face::Regular,
            BODY,
            BODY_LINE,
            MARGIN,
            214.0,
            width,
            color::text(),
            &error_text(&error.kind),
        );
        self.text(
            Face::Medium,
            SMALL,
            SMALL_LINE,
            MARGIN,
            y + 20.0,
            color::muted(),
            &t("details"),
        );
        let y = self.paragraph(
            Face::Mono,
            SMALL - 2.0,
            SMALL_LINE - 2.0,
            MARGIN,
            y + 20.0 + SMALL_LINE,
            width,
            color::muted(),
            &error.message.chars().take(400).collect::<String>(),
        );
        let top = (y + 36.0).max(400.0).min(self.ui.h - 110.0);
        let w = self.fit(
            Event::Action(RoomAction::Retry),
            &t("retry"),
            MARGIN,
            top,
            80.0,
            Look::Primary,
        );
        let mut x = MARGIN + w + 20.0;
        if model.sounds_stage {
            x += 20.0
                + self.fit(
                    Event::Action(RoomAction::SetSounds(false)),
                    &t("download.skip"),
                    x,
                    top,
                    80.0,
                    Look::Secondary,
                );
        }
        if model.account.is_some() {
            self.fit(
                Event::Action(RoomAction::SignOut),
                &t("sign_out"),
                x,
                top,
                80.0,
                Look::Secondary,
            );
        }
    }

    fn idle(&mut self, model: &RoomModel) {
        let w = self.ui.w;
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            120.0,
            color::text(),
            &tf("idle.version", &[&model.version]),
        );
        let (status, ink) = if model.verifying {
            (t("idle.checking"), color::muted())
        } else if model.offline {
            (t("idle.offline"), color::muted())
        } else if let Some(name) = &model.account {
            (tf("idle.signed_in", &[name]), good())
        } else {
            (t("idle.not_signed_in"), color::muted())
        };
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            190.0,
            ink,
            &status,
        );
        self.button(
            Event::Action(RoomAction::Play),
            &t("idle.play"),
            [MARGIN, 250.0, 380.0, 104.0],
            if model.can_play {
                Look::Primary
            } else {
                Look::Off
            },
            42.0,
        );
        let x = MARGIN
            + self.fit(
                Event::Settings(true),
                &t("idle.settings"),
                MARGIN,
                400.0,
                72.0,
                Look::Secondary,
            )
            + 20.0;
        self.fit(
            Event::Action(RoomAction::SignOut),
            &t("sign_out"),
            x,
            400.0,
            72.0,
            if model.account.is_some() {
                Look::Secondary
            } else {
                Look::Off
            },
        );
        // The news card.
        let card_w = 430.0_f32.min(w * 0.38);
        let (cx, cy, ch) = (w - MARGIN - card_w, 128.0, self.ui.h - 128.0 - 56.0);
        self.ui.rounded(cx, cy, card_w, ch, 20.0, color::raised());
        self.text(
            Face::Medium,
            SMALL,
            SMALL_LINE,
            cx + 28.0,
            cy + 24.0,
            color::muted(),
            &t("idle.news"),
        );
        let inner = card_w - 56.0;
        match &model.news {
            Some(news) => {
                let mut y = self.paragraph(
                    Face::SemiBold,
                    28.0,
                    38.0,
                    cx + 28.0,
                    cy + 64.0,
                    inner,
                    color::text(),
                    &news.title,
                );
                if !news.date.is_empty() {
                    self.text(
                        Face::Regular,
                        SMALL - 2.0,
                        SMALL_LINE,
                        cx + 28.0,
                        y,
                        color::muted(),
                        &news.date,
                    );
                    y += SMALL_LINE + 10.0;
                }
                for line in &news.lines {
                    if y + BODY_LINE > cy + ch - 20.0 {
                        break;
                    }
                    y = self.paragraph(
                        Face::Regular,
                        SMALL,
                        SMALL_LINE,
                        cx + 28.0,
                        y,
                        inner,
                        color::text(),
                        line,
                    ) + 8.0;
                }
            }
            None => {
                self.paragraph(
                    Face::Regular,
                    SMALL,
                    SMALL_LINE,
                    cx + 28.0,
                    cy + 70.0,
                    inner,
                    color::muted(),
                    &t("idle.no_news"),
                );
            }
        }
    }

    fn toggle(&mut self, event: Event, on: bool, x: f32, y: f32) {
        let label = if on {
            t("settings.on")
        } else {
            t("settings.off")
        };
        self.button(
            event,
            &label,
            [x, y, 150.0, 64.0],
            if on { Look::Primary } else { Look::Secondary },
            BUTTON_TEXT,
        );
    }

    fn settings(&mut self, model: &RoomModel) {
        let w = self.ui.w;
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            120.0,
            color::text(),
            &t("settings.title"),
        );
        let right = w - MARGIN - 150.0;
        self.text(
            Face::Regular,
            BODY,
            64.0,
            MARGIN,
            210.0,
            color::text(),
            &t("settings.sounds"),
        );
        self.toggle(
            Event::Action(RoomAction::SetSounds(!model.sounds)),
            model.sounds,
            right,
            210.0,
        );
        self.ui
            .rounded(MARGIN, 290.0, w - 2.0 * MARGIN, 1.0, 0.0, color::border());
        match &model.system_language {
            Some(code) => {
                self.text(
                    Face::Regular,
                    BODY,
                    64.0,
                    MARGIN,
                    310.0,
                    color::text(),
                    &tf("settings.language", &[code]),
                );
                self.toggle(
                    Event::Action(RoomAction::SetSystemLanguage(!model.use_system_language)),
                    model.use_system_language,
                    right,
                    310.0,
                );
            }
            None => {
                self.text(
                    Face::Regular,
                    BODY,
                    64.0,
                    MARGIN,
                    310.0,
                    color::muted(),
                    &t("settings.language_none"),
                );
            }
        }
        self.fit(
            Event::Graphics(true),
            &t("settings.graphics"),
            MARGIN,
            420.0,
            72.0,
            Look::Secondary,
        );
        self.fit(
            Event::Settings(false),
            &t("settings.back"),
            MARGIN,
            520.0,
            72.0,
            Look::Secondary,
        );
    }
    fn graphics_extras(&mut self) {
        use horizon_client_input::{headset, headset_display};
        let settings = headset::settings();
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            120.0,
            color::text(),
            &t("graphics.extras"),
        );
        self.fit(
            Event::DynamicResolution,
            &t(if settings.bool("horizonDynamicResolution") {
                "graphics.dynamic.on"
            } else {
                "graphics.dynamic.off"
            }),
            MARGIN,
            205.0,
            48.0,
            Look::Secondary,
        );
        let supported = headset_display::snapshot().is_some_and(|i| i.super_resolution_supported);
        self.fit(
            Event::SuperResolution,
            &t(if !supported {
                "graphics.mqsr.unavailable"
            } else if settings.bool("horizonSuperResolution") {
                "graphics.mqsr.on"
            } else {
                "graphics.mqsr.off"
            }),
            MARGIN,
            265.0,
            48.0,
            if supported {
                Look::Secondary
            } else {
                Look::Off
            },
        );
        let ffr = match settings.enum_name("horizonFoveation") {
            "LOW" => "graphics.ffr.low",
            "MEDIUM" => "graphics.ffr.medium",
            "HIGH" => "graphics.ffr.high",
            _ => "graphics.ffr.off",
        };
        self.fit(
            Event::Foveation,
            &t(ffr),
            MARGIN,
            325.0,
            48.0,
            Look::Secondary,
        );
        self.fit(
            Event::OpaqueWater,
            &t(if settings.bool("horizonOpaqueWater") {
                "graphics.water.on"
            } else {
                "graphics.water.off"
            }),
            MARGIN,
            385.0,
            48.0,
            Look::Secondary,
        );
        self.fit(
            Event::FoveatedMultiview,
            &t(if settings.bool("horizonFoveatedMultiview") {
                "graphics.foveated_multiview.on"
            } else {
                "graphics.foveated_multiview.off"
            }),
            MARGIN,
            445.0,
            48.0,
            Look::Secondary,
        );
        for (y, key) in [
            (520.0, "graphics.dynamic.note"),
            (555.0, "graphics.mqsr.note"),
            (590.0, "graphics.ffr.note"),
        ] {
            self.text(
                Face::Regular,
                SMALL,
                SMALL_LINE,
                MARGIN,
                y,
                color::muted(),
                &t(key),
            );
        }
        self.fit(
            Event::GraphicsExtras(false),
            &t("settings.back"),
            MARGIN,
            645.0,
            48.0,
            Look::Secondary,
        );
    }
    fn graphics(&mut self) {
        use horizon_client_input::{headset, headset_display};
        let settings = headset::settings();
        let info = headset_display::snapshot();
        let scale = info
            .as_ref()
            .and_then(|i| i.scale_override)
            .unwrap_or_else(|| settings.float("renderScaleFactor"));
        self.text(
            Face::SemiBold,
            H1,
            64.0,
            MARGIN,
            120.0,
            color::text(),
            &t("graphics.title"),
        );
        let label = info.as_ref().map_or_else(
            || tf("graphics.scale", &[&(scale * 100.0).round()]),
            |i| {
                let size = headset_display::eye_size(i.recommended, i.maximum, scale);
                tf(
                    "graphics.per_eye",
                    &[&size[0], &size[1], &(scale * 100.0).round()],
                )
            },
        );
        self.text(
            Face::Regular,
            BODY,
            64.0,
            MARGIN,
            210.0,
            color::text(),
            &t("graphics.resolution"),
        );
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            265.0,
            color::muted(),
            &label,
        );
        let right = self.ui.w - MARGIN - 170.0;
        let changeable = info.as_ref().is_none_or(|i| i.scale_override.is_none());
        for (delta, x, label) in [(-1, right, "−"), (1, right + 90.0, "+")] {
            self.button(
                Event::RenderScale(delta),
                label,
                [x, 210.0, 72.0, 64.0],
                if changeable && headset_display::step_scale(scale, delta) != scale {
                    Look::Secondary
                } else {
                    Look::Off
                },
                32.0,
            );
        }
        let cadence = horizon_vivecraft::refresh::requested(settings.float("horizonRefreshRate"));
        let hz = cadence.render;
        self.text(
            Face::Regular,
            BODY,
            64.0,
            MARGIN,
            330.0,
            color::text(),
            &if cadence.spacewarp {
                tf(
                    "graphics.spacewarp",
                    &[&cadence.render.round(), &cadence.display.round()],
                )
            } else {
                tf("graphics.refresh", &[&cadence.display.round()])
            },
        );
        for (delta, x, label) in [(-1, right, "−"), (1, right + 90.0, "+")] {
            let can = info.as_ref().is_some_and(|i| {
                headset_display::step_rate(hz, delta, &headset_display::render_rates(i)) != hz
            });
            self.button(
                Event::RefreshRate(delta),
                label,
                [x, 330.0, 72.0, 64.0],
                if can { Look::Secondary } else { Look::Off },
                32.0,
            );
        }
        let mut display = info.as_ref().and_then(|i| i.current_hz).map_or_else(
            || t("graphics.unknown"),
            |hz| tf("graphics.display", &[&hz.round()]),
        );
        if cadence.spacewarp {
            display.push_str(" · ");
            display.push_str(&t(
                if info.as_ref().is_some_and(|i| !i.spacewarp_supported) {
                    "graphics.spacewarp.unavailable"
                } else if info.as_ref().is_some_and(|i| i.spacewarp_active) {
                    "graphics.spacewarp.active"
                } else {
                    "graphics.spacewarp.native"
                },
            ));
        }
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            395.0,
            color::muted(),
            &display,
        );
        self.fit(
            Event::Multiview,
            &t(if settings.bool("horizonMultiview") {
                "graphics.multiview.on"
            } else {
                "graphics.multiview.off"
            }),
            MARGIN,
            425.0,
            64.0,
            Look::Secondary,
        );
        self.fit(
            Event::GraphicsExtras(true),
            &t("graphics.extras"),
            MARGIN,
            495.0,
            58.0,
            Look::Secondary,
        );
        let note = if changeable {
            t("graphics.apply")
        } else {
            t("graphics.override")
        };
        self.text(
            Face::Regular,
            SMALL,
            SMALL_LINE,
            MARGIN,
            570.0,
            color::muted(),
            &note,
        );
        self.fit(
            Event::Graphics(false),
            &t("settings.back"),
            MARGIN,
            620.0,
            72.0,
            Look::Secondary,
        );
    }
}

impl RoomUi {
    /// Draws the TV's page for `model` into `batch`; `pointer` is where the ray meets the panel
    /// (0..1 from its top left) and `down` the trigger. Returns what a press that ended on a
    /// button asked for (a press that began and ended on the same button), and whether the
    /// pointer moved onto a different button (for a tick of haptics).
    pub fn frame(
        &mut self,
        atlas: &HudAtlas,
        batch: &mut HudBatch,
        model: &RoomModel,
        pointer: Option<[f32; 2]>,
        down: bool,
    ) -> (Option<Event>, bool) {
        // The qr code of the page's address, made once.
        if let Some(code) = model.code.as_ref().filter(|_| model.page == Page::SignIn)
            && self.qr.as_ref().is_none_or(|(url, _)| *url != code.qr_url)
        {
            self.qr = qr_modules(&code.qr_url).map(|m| (code.qr_url.clone(), m));
        }
        let cursor_gui = pointer.map(|[u, v]| (u * batch.width, v * batch.height));
        let mut ui = Ui::new(atlas, batch, cursor_gui.unwrap_or((-1.0e4, -1.0e4)));
        let cursor = cursor_gui.map(|_| ui.cursor);
        let mut hits = Vec::new();
        {
            let mut canvas = Canvas {
                ui: &mut ui,
                hits: &mut hits,
                cursor,
                pressed: self.pressed,
            };
            let (w, h) = (canvas.ui.w, canvas.ui.h);
            canvas.ui.rounded(0.0, 0.0, w, h, 0.0, color::bg());
            canvas.header(model);
            if model.error.is_some() {
                canvas.error(model);
            } else {
                match model.page {
                    Page::NoAccount => canvas.no_account(model),
                    Page::SignIn => {
                        let qr = self.qr.as_ref().map(|(_, m)| m.as_slice()).unwrap_or(&[]);
                        canvas.sign_in(model, qr);
                    }
                    Page::Checking => canvas.checking(model),
                    Page::Download => canvas.download(model),
                    Page::Prepare => canvas.prepare(model),
                    Page::Idle if self.settings && self.graphics && self.graphics_extras => {
                        canvas.graphics_extras()
                    }
                    Page::Idle if self.settings && self.graphics => canvas.graphics(),
                    Page::Idle if self.settings => canvas.settings(model),
                    Page::Idle => canvas.idle(model),
                }
            }
        }
        // The cursor where the ray meets the TV (the game's menus draw theirs on their panel): a
        // white dot in a dark ring, accent while the trigger is down, over the page.
        if let Some((x, y)) = cursor {
            ui.rounded(
                x - CURSOR_RADIUS - 1.5,
                y - CURSOR_RADIUS - 1.5,
                2.0 * CURSOR_RADIUS + 3.0,
                2.0 * CURSOR_RADIUS + 3.0,
                CURSOR_RADIUS + 1.5,
                0xFF0D_0E13,
            );
            let fill = if down { color::accent() } else { 0xFFFF_FFFF };
            ui.rounded(
                x - CURSOR_RADIUS,
                y - CURSOR_RADIUS,
                2.0 * CURSOR_RADIUS,
                2.0 * CURSOR_RADIUS,
                CURSOR_RADIUS,
                fill,
            );
        }
        ui.finish();
        self.hits = hits;
        if model.page != Page::Idle || model.error.is_some() {
            self.settings = false;
        }
        // Presses: a trigger that went down on a button and came up on the same one.
        let under = cursor.and_then(|c| self.hits.iter().find(|h| h.contains(c)).map(|h| h.event));
        let mut event = None;
        if down && !self.was_down {
            self.pressed = under;
        } else if !down && self.was_down {
            if under.is_some() && under == self.pressed {
                event = under;
            }
            self.pressed = None;
        }
        self.was_down = down;
        let moved = under.is_some() && under != self.hover;
        self.hover = under;
        if let Some(Event::Settings(open)) = event {
            self.settings = open;
            if !open {
                self.graphics = false;
            }
        }
        if let Some(Event::Graphics(open)) = event {
            self.graphics = open;
            self.graphics_extras = false;
        }
        if let Some(Event::GraphicsExtras(open)) = event {
            self.graphics_extras = open;
        }
        (event, moved)
    }

    /// The rectangles of the buttons drawn last frame, in dp, with what each does.
    pub fn buttons(&self) -> Vec<([f32; 4], Event)> {
        self.hits.iter().map(|h| (h.rect, h.event)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Language;
    use horizon_client_input::room::{Code, Failure, News, Progress};

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn idle() -> RoomModel {
        RoomModel {
            page: Page::Idle,
            account: Some("Notch".into()),
            version: "26.3".into(),
            can_play: true,
            news: Some(News {
                title: "Nightly build".into(),
                date: "2026-11-07".into(),
                lines: vec!["The headset launcher shows this news line on the TV".into()],
            }),
            system_language: Some("ru_ru".into()),
            ..RoomModel::default()
        }
    }

    fn signing_in() -> RoomModel {
        RoomModel {
            page: Page::SignIn,
            code: Some(Code {
                user_code: "ABCD1234".into(),
                url: "https://www.microsoft.com/link".into(),
                qr_url: "https://www.microsoft.com/link?otc=ABCD1234".into(),
                expires_in: 899,
            }),
            ..RoomModel::default()
        }
    }

    fn downloading() -> RoomModel {
        RoomModel {
            page: Page::Download,
            download: Some(Progress {
                files_done: 12,
                files_total: 200,
                bytes_done: 40 << 20,
                bytes_total: 197 << 20,
                bytes_per_sec: 3.0e6,
                eta_secs: Some(52),
            }),
            ..RoomModel::default()
        }
    }

    fn failed() -> RoomModel {
        RoomModel {
            page: Page::Download,
            account: Some("Notch".into()),
            error: Some(Failure {
                kind: "network".into(),
                message: "https://piston-data.mojang.com/v1/objects/x timed out".into(),
            }),
            ..RoomModel::default()
        }
    }

    /// Every page the room has, with the language it is drawn in.
    fn pages() -> Vec<(&'static str, RoomModel)> {
        vec![
            ("no_account", RoomModel::default()),
            ("sign_in", signing_in()),
            (
                "checking",
                RoomModel {
                    page: Page::Checking,
                    step: Some("xbox".into()),
                    account: Some("Notch".into()),
                    ..RoomModel::default()
                },
            ),
            ("download", downloading()),
            (
                "prepare",
                RoomModel {
                    page: Page::Prepare,
                    prepare: Some(("Exporting server data".into(), 0.4)),
                    ..RoomModel::default()
                },
            ),
            ("error", failed()),
            ("idle", idle()),
        ]
    }

    #[test]
    fn the_pointer_is_drawn_as_a_dot_where_it_is_and_nowhere_without_one() {
        let a = atlas();
        let model = idle();
        let draw = |pointer: Option<[f32; 2]>, down: bool| {
            let mut batch = HudBatch::new([1280, 720]);
            RoomUi::default().frame(&a, &mut batch, &model, pointer, down);
            batch
        };
        let none = draw(None, false);
        let on_empty = draw(Some([0.5, 0.9]), false);
        let on_empty_down = draw(Some([0.5, 0.9]), true);
        // The dot is more geometry, and it is where the pointer is: a vertex near (0.5, 0.9).
        assert!(on_empty.vertices.len() > none.vertices.len());
        let near = |batch: &HudBatch| {
            batch
                .vertices
                .iter()
                .filter(|v| {
                    (v.pos[0] - 0.5 * batch.width).abs() < 14.0
                        && (v.pos[1] - 0.9 * batch.height).abs() < 14.0
                })
                .count()
        };
        assert!(near(&on_empty) > near(&none));
        // Pressed it takes the accent colour instead of white.
        let colours = |batch: &HudBatch| {
            batch
                .vertices
                .iter()
                .filter(|v| {
                    (v.pos[0] - 0.5 * batch.width).abs() < 3.0
                        && (v.pos[1] - 0.9 * batch.height).abs() < 3.0
                })
                .map(|v| v.color)
                .collect::<std::collections::HashSet<_>>()
        };
        assert_ne!(colours(&on_empty), colours(&on_empty_down));
    }

    #[test]
    fn every_key_the_room_draws_has_an_english_and_a_russian_text() {
        for code in ["en_us", "ru_ru"] {
            let (texts, missed) = lang::scoped(Language::load(&[], code), || {
                KEYS.iter().map(|k| t(k)).collect::<Vec<_>>()
            });
            assert!(missed.is_empty(), "{code}: {missed:?}");
            assert!(texts.iter().all(|s| !s.is_empty()), "{code}");
        }
        // The room draws nothing the list does not name: drawing every page asks for no other key.
        for code in ["en_us", "ru_ru"] {
            let (_, missed) = lang::scoped(Language::load(&[], code), || {
                for (_, model) in pages() {
                    let mut batch = HudBatch::new([1280, 720]);
                    RoomUi::default().frame(&atlas(), &mut batch, &model, None, false);
                }
            });
            assert!(missed.is_empty(), "{code}: {missed:?}");
        }
        // The headline of every error kind the launcher can name.
        let (_, missed) = lang::scoped(Language::load(&[], "en_us"), || {
            for kind in [
                "network",
                "not_owner",
                "no_profile",
                "not_approved",
                "not_configured",
                "xbox_no_profile",
                "xbox_region",
                "xbox_adult",
                "xbox_child",
                "code_expired",
                "declined",
                "sign_in_expired",
                "no_space",
                "checksum",
                "cancelled",
                "jvm_failed",
                "other",
                "a kind nobody named",
            ] {
                error_text(kind);
            }
        });
        assert!(missed.is_empty(), "{missed:?}");
    }

    #[test]
    fn the_russian_pages_still_fit_where_the_english_ones_do() {
        // Longer words must not push a button off the page: every button stays inside it.
        for code in ["en_us", "ru_ru"] {
            lang::scoped(Language::load(&[], code), || {
                for (name, model) in pages() {
                    let mut batch = HudBatch::new([1280, 720]);
                    let mut room = RoomUi::default();
                    room.frame(&atlas(), &mut batch, &model, None, false);
                    let a = atlas();
                    let ui = Ui::new(&a, &mut batch, (0.0, 0.0));
                    for ([x, y, w, h], _) in room.buttons() {
                        assert!(
                            x >= 0.0 && y >= 0.0 && x + w <= ui.w && y + h <= ui.h,
                            "{code} {name}: {x} {y} {w} {h} in {}x{}",
                            ui.w,
                            ui.h
                        );
                    }
                }
            });
        }
    }

    /// Presses at `at` (0..1) and releases there: what the page answers.
    fn click(room: &mut RoomUi, model: &RoomModel, at: [f32; 2]) -> Option<Event> {
        let a = atlas();
        let mut batch = HudBatch::new([1280, 720]);
        room.frame(&a, &mut batch, model, Some(at), false);
        room.frame(&a, &mut HudBatch::new([1280, 720]), model, Some(at), true);
        room.frame(&a, &mut HudBatch::new([1280, 720]), model, Some(at), false)
            .0
    }

    fn centre_of(room: &RoomUi, event: Event) -> [f32; 2] {
        let ([x, y, w, h], _) = room
            .buttons()
            .into_iter()
            .find(|(_, e)| *e == event)
            .unwrap_or_else(|| panic!("no {event:?} among {:?}", room.buttons()));
        // dp to the panel's 0..1: the density of a 1280 by 720 panel.
        let batch = HudBatch::new([1280, 720]);
        let density = (batch.width * batch.scale / 1040.0)
            .min(batch.height * batch.scale / 700.0)
            .clamp(1.0, 4.0);
        [
            (x + w / 2.0) * density / (batch.width * batch.scale),
            (y + h / 2.0) * density / (batch.height * batch.scale),
        ]
    }

    #[test]
    fn each_button_answers_a_press_where_it_is_drawn_and_only_there() {
        lang::scoped(Language::load(&[], "en_us"), || {
            let sounds_failed = RoomModel {
                sounds_stage: true,
                ..failed()
            };
            let cases: Vec<(RoomModel, Event)> = vec![
                (RoomModel::default(), Event::Action(RoomAction::SignIn)),
                (signing_in(), Event::Action(RoomAction::CancelSignIn)),
                (downloading(), Event::Action(RoomAction::CancelDownload)),
                (failed(), Event::Action(RoomAction::Retry)),
                (failed(), Event::Action(RoomAction::SignOut)),
                (
                    sounds_failed.clone(),
                    Event::Action(RoomAction::SetSounds(false)),
                ),
                (sounds_failed.clone(), Event::Action(RoomAction::SignOut)),
                (idle(), Event::Action(RoomAction::Play)),
                (idle(), Event::Action(RoomAction::SignOut)),
                (idle(), Event::Settings(true)),
            ];
            for (model, event) in cases {
                let mut room = RoomUi::default();
                let mut batch = HudBatch::new([1280, 720]);
                room.frame(&atlas(), &mut batch, &model, None, false);
                let at = centre_of(&room, event);
                assert_eq!(click(&mut room, &model, at), Some(event), "{event:?}");
            }
            // Pressing on the page's empty corner does nothing.
            let mut room = RoomUi::default();
            assert_eq!(click(&mut room, &idle(), [0.01, 0.97]), None);
        });
    }

    #[test]
    fn a_press_that_slides_off_its_button_before_the_release_does_nothing() {
        lang::scoped(Language::load(&[], "en_us"), || {
            let model = idle();
            let mut room = RoomUi::default();
            let a = atlas();
            room.frame(&a, &mut HudBatch::new([1280, 720]), &model, None, false);
            let play = centre_of(&room, Event::Action(RoomAction::Play));
            room.frame(
                &a,
                &mut HudBatch::new([1280, 720]),
                &model,
                Some(play),
                false,
            );
            room.frame(
                &a,
                &mut HudBatch::new([1280, 720]),
                &model,
                Some(play),
                true,
            );
            let (event, _) = room.frame(
                &a,
                &mut HudBatch::new([1280, 720]),
                &model,
                Some([0.01, 0.97]),
                false,
            );
            assert_eq!(event, None);
        });
    }

    #[test]
    fn play_cannot_be_pressed_until_the_launcher_says_so() {
        lang::scoped(Language::load(&[], "en_us"), || {
            let model = RoomModel {
                can_play: false,
                ..idle()
            };
            let mut room = RoomUi::default();
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            assert!(
                room.buttons()
                    .iter()
                    .all(|(_, e)| *e != Event::Action(RoomAction::Play)),
                "{:?}",
                room.buttons()
            );
        });
    }

    #[test]
    fn settings_open_and_close_on_the_room_itself_and_toggles_ask_the_launcher() {
        lang::scoped(Language::load(&[], "en_us"), || {
            let model = idle();
            let mut room = RoomUi::default();
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            let at = centre_of(&room, Event::Settings(true));
            assert_eq!(click(&mut room, &model, at), Some(Event::Settings(true)));
            // The next frame is the settings page: sounds on becomes off, the language too.
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            let sounds = centre_of(&room, Event::Action(RoomAction::SetSounds(false)));
            assert_eq!(
                click(&mut room, &model, sounds),
                Some(Event::Action(RoomAction::SetSounds(false)))
            );
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            let language = centre_of(&room, Event::Action(RoomAction::SetSystemLanguage(false)));
            assert_eq!(
                click(&mut room, &model, language),
                Some(Event::Action(RoomAction::SetSystemLanguage(false)))
            );
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            let back = centre_of(&room, Event::Settings(false));
            assert_eq!(click(&mut room, &model, back), Some(Event::Settings(false)));
            room.frame(
                &atlas(),
                &mut HudBatch::new([1280, 720]),
                &model,
                None,
                false,
            );
            assert!(
                room.buttons()
                    .iter()
                    .any(|(_, e)| *e == Event::Action(RoomAction::Play))
            );
        });
    }

    #[test]
    fn the_qr_code_is_a_real_one_with_the_three_finder_squares() {
        let m = qr_modules("https://www.microsoft.com/link?otc=ABCD1234").unwrap();
        let n = m.len();
        assert!((21..=45).contains(&n), "{n}");
        assert!(m.iter().all(|row| row.len() == n));
        // A finder pattern: a dark ring, a light ring, a dark 3 by 3 centre, in three corners.
        let finder = |x0: usize, y0: usize| {
            (0..7).all(|d| m[y0][x0 + d] && m[y0 + 6][x0 + d] && m[y0 + d][x0] && m[y0 + d][x0 + 6])
                && (1..6).all(|d| {
                    !m[y0 + 1][x0 + d]
                        && !m[y0 + 5][x0 + d]
                        && !m[y0 + d][x0 + 1]
                        && !m[y0 + d][x0 + 5]
                })
                && (2..5).all(|y| (2..5).all(|x| m[y0 + y][x0 + x]))
        };
        assert!(finder(0, 0) && finder(n - 7, 0) && finder(0, n - 7));
        assert!(!finder(n - 7, n - 7), "the fourth corner has none");
    }

    #[test]
    fn times_and_sizes_read_the_way_a_download_shows_them() {
        assert_eq!(clock(52), "0:52");
        assert_eq!(clock(899), "14:59");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(megabytes(40 << 20), "40");
        assert_eq!(megabytes(3 << 20), "3.0");
    }
}
