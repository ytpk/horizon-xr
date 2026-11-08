//! What a program that embeds the game hands it before it starts: the command
//! line, where the process has none of its own (an Android app's
//! `android_main`), and on Android the activity the window lives in.
use std::sync::OnceLock;
use winit::event_loop::EventLoop;

static ARGS: OnceLock<Vec<String>> = OnceLock::new();

/// The game's command line from now on, in place of the process's (once).
pub fn set_args(args: Vec<String>) {
    let _ = ARGS.set(args);
}

/// The command line without the program's name: the embedder's when it gave
/// one, else the process's.
pub fn args() -> Vec<String> {
    match ARGS.get() {
        Some(args) => args.clone(),
        None => std::env::args().skip(1).collect(),
    }
}

#[cfg(target_os = "android")]
static ANDROID_APP: OnceLock<winit::platform::android::activity::AndroidApp> = OnceLock::new();

/// The activity the game's event loop runs in (once, from `android_main`).
#[cfg(target_os = "android")]
pub fn set_android_app(app: winit::platform::android::activity::AndroidApp) {
    let _ = ANDROID_APP.set(app);
}

/// The event loop the game runs on: on Android, in the embedder's activity.
pub fn event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let mut builder = EventLoop::builder();
        if let Some(app) = ANDROID_APP.get() {
            builder.with_android_app(app.clone());
        }
        builder.build()
    }
    #[cfg(not(target_os = "android"))]
    EventLoop::new()
}
