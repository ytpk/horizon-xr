//! Android runtime permissions for the headset's body and hand tracking. The manifest names them
//! (`com.oculus.permission.BODY_TRACKING`, `com.oculus.permission.HAND_TRACKING`), but the system only
//! grants them once the app asks: the first time a tracker is refused for the missing permission, the
//! game asks here and the system shows its own prompt in the headset.

/// The permission body tracking (`XR_FB_body_tracking`) needs.
pub const BODY_TRACKING: &str = "com.oculus.permission.BODY_TRACKING";
/// The permission hand tracking (`XR_EXT_hand_tracking`) needs.
pub const HAND_TRACKING: &str = "com.oculus.permission.HAND_TRACKING";

/// Asks the activity for `name` unless it already has it; true when it was already granted. The
/// system's prompt answers later: nothing here waits for it.
#[cfg(target_os = "android")]
pub fn request(name: &str) -> bool {
    use jni::{JavaVM, objects::JObject};
    let context = ndk_context::android_context();
    if context.vm().is_null() || context.context().is_null() {
        log::warn!("Horizon XR: no activity to ask for {name}");
        return false;
    }
    // Safety: android-activity gave the process its VM and the activity (the quest crate makes the
    // activity the context); both live as long as the process.
    let vm = match unsafe { JavaVM::from_raw(context.vm().cast()) } {
        Ok(vm) => vm,
        Err(e) => {
            log::warn!("Horizon XR: the Java VM for {name}: {e}");
            return false;
        }
    };
    let result = (|| -> jni::errors::Result<bool> {
        let mut env = vm.attach_current_thread()?;
        // Safety: as above, a live reference the activity holds.
        let activity = unsafe { JObject::from_raw(context.context().cast()) };
        let permission = env.new_string(name)?;
        let state = env
            .call_method(
                &activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[(&permission).into()],
            )?
            .i()?;
        if state == 0 {
            return Ok(true);
        }
        let list = env.new_object_array(1, "java/lang/String", &permission)?;
        env.call_method(
            &activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[(&list).into(), 4242.into()],
        )?;
        Ok(false)
    })();
    match result {
        Ok(granted) => {
            if !granted {
                log::info!("Horizon XR: asked the system for {name}");
            }
            granted
        }
        Err(e) => {
            log::warn!("Horizon XR: asking for {name}: {e}");
            false
        }
    }
}

/// Without Android there is nothing to ask.
#[cfg(not(target_os = "android"))]
pub fn request(_name: &str) -> bool {
    true
}
