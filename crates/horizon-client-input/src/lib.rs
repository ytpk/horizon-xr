//! The client's input layer, below gameplay and screens: key bindings and their
//! names, toggle keys, mouse settings, controllers, the clipboard and window
//! options. horizon-client re-exports each module under its old path
//! (`horizon_client::keybinds`, ...).
#![forbid(unsafe_code)]
pub mod clipboard;
pub mod gamepad;
pub mod headset;
pub mod headset_display;
pub mod input;
pub mod input_mode;
pub mod interaction_input;
pub mod key_names;
pub mod keybinds;
pub mod keymapping;
pub mod modifiers;
pub mod mouse;
pub mod pointer_route;
pub mod room;
pub mod window;

/// Quest gameplay bindings; menu interaction retains a fixed recovery layout.
pub mod vr_bindings;
