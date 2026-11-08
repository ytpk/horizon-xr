//! `horizon-jvm`: runs a Java program the way `java` does (`-cp`, `-jar`, `-D`, `-X`, `-XX`,
//! `--add-opens`, a main class and its arguments, `-version`), by loading `libjvm.so` and calling
//! `JNI_CreateJavaVM` itself, in its own process.
//!
//! Why not the JDK's `java`: its launcher finds the runtime from where its own file is, and on
//! Android an app's executables live in the flat native library folder, which is not a JDK
//! layout. HotSpot finds `lib/modules` from where `libjvm.so` is, so this launcher opens it at
//! `<home>/lib/server/libjvm.so`. The home is `HORIZON_JVM_HOME`, else the folder above this
//! program's `bin`. A separate process (not the app's own) keeps a JVM crash from taking the
//! app down, and `horizon-prepare` already runs its JVMs that way.
use jni_sys::{
    jboolean, jclass, jint, jobject, jstring, JNIEnv, JavaVM, JavaVMInitArgs, JavaVMOption,
    JNI_VERSION_10,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::process::ExitCode;

type CreateVm = unsafe extern "system" fn(*mut *mut JavaVM, *mut *mut c_void, *mut c_void) -> jint;

/// What the command line asks for.
#[derive(Debug, PartialEq, Default)]
struct Plan {
    options: Vec<String>,
    class_path: Option<String>,
    jar: Option<String>,
    main: Option<String>,
    args: Vec<String>,
    version: bool,
}

/// Reads a `java` command line. Options that take a value as the next argument are the ones the
/// preparer and the exporters use.
fn parse(args: &[String]) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-version" | "--version" => plan.version = true,
            "-cp" | "-classpath" | "--class-path" => {
                i += 1;
                plan.class_path = Some(args.get(i).ok_or("-cp needs a value")?.clone());
            }
            "-jar" => {
                i += 1;
                plan.jar = Some(args.get(i).ok_or("-jar needs a file")?.clone());
                plan.args = args[i + 1..].to_vec();
                return Ok(plan);
            }
            "--add-opens"
            | "--add-exports"
            | "--add-modules"
            | "--add-reads"
            | "--module-path"
            | "-p"
            | "--enable-native-access"
            | "--limit-modules"
            | "--patch-module" => {
                i += 1;
                let value = args.get(i).ok_or(format!("{a} needs a value"))?;
                let long = if a == "-p" { "--module-path" } else { a };
                plan.options.push(format!("{long}={value}"));
            }
            _ if a.starts_with("--") && a.contains('=') => plan.options.push(a.to_string()),
            _ if a.starts_with('-') => plan.options.push(a.to_string()),
            _ => {
                plan.main = Some(a.to_string());
                plan.args = args[i + 1..].to_vec();
                return Ok(plan);
            }
        }
        i += 1;
    }
    Ok(plan)
}

/// The `Main-Class` of a jar's manifest.
fn jar_main(path: &str) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{path}: {e}"))?;
    let mut manifest = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name("META-INF/MANIFEST.MF")
            .map_err(|e| format!("{path}: {e}"))?,
        &mut manifest,
    )
    .map_err(|e| format!("{path}: {e}"))?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:"))
        .map(|c| c.trim().to_string())
        .ok_or_else(|| format!("no main manifest attribute, in {path}"))
}

fn libjvm() -> Result<PathBuf, String> {
    let home = match std::env::var_os("HORIZON_JVM_HOME").filter(|h| !h.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.to_path_buf()))
            .ok_or("HORIZON_JVM_HOME is not set")?,
    };
    let lib = home.join("lib/server/libjvm.so");
    if lib.is_file() {
        Ok(lib)
    } else {
        Err(format!(
            "{} not found (set HORIZON_JVM_HOME)",
            lib.display()
        ))
    }
}

/// The thread stack the main Java thread gets: `-Xss`, else 8 MB (the JDK launcher's thread is
/// as big as the VM's default stack; ours is generous because Minecraft's codecs recurse deeply).
fn stack_size(options: &[String]) -> usize {
    options
        .iter()
        .rev()
        .find_map(|o| o.strip_prefix("-Xss"))
        .and_then(|v| {
            let (digits, unit) =
                v.split_at(v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len()));
            let n: usize = digits.parse().ok()?;
            Some(match unit.to_ascii_lowercase().as_str() {
                "k" => n << 10,
                "m" => n << 20,
                "g" => n << 30,
                _ => n,
            })
        })
        .unwrap_or(8 << 20)
        .max(1 << 20)
}

fn run(plan: Plan) -> Result<i32, String> {
    let libjvm = libjvm()?;
    let path = CString::new(libjvm.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: loading the JVM; its entry point has the JNI invocation signature.
    let create: CreateVm = unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            let why = CStr::from_ptr(libc::dlerror())
                .to_string_lossy()
                .into_owned();
            return Err(format!("{}: {why}", libjvm.display()));
        }
        let symbol = libc::dlsym(handle, c"JNI_CreateJavaVM".as_ptr());
        if symbol.is_null() {
            return Err("libjvm has no JNI_CreateJavaVM".into());
        }
        std::mem::transmute::<*mut c_void, CreateVm>(symbol)
    };
    let mut options = plan.options.clone();
    if let Some(jar) = &plan.jar {
        options.push(format!("-Djava.class.path={jar}"));
    } else if let Some(cp) = &plan.class_path {
        options.push(format!("-Djava.class.path={cp}"));
    }
    options.push("-Dsun.java.launcher=SUN_STANDARD".into());
    let strings: Vec<CString> = options
        .iter()
        .map(|o| CString::new(o.as_str()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let mut raw: Vec<JavaVMOption> = strings
        .iter()
        .map(|s| JavaVMOption {
            optionString: s.as_ptr() as *mut c_char,
            extraInfo: std::ptr::null_mut(),
        })
        .collect();
    let mut init = JavaVMInitArgs {
        version: JNI_VERSION_10,
        nOptions: raw.len() as jint,
        options: raw.as_mut_ptr(),
        ignoreUnrecognized: 1 as jboolean,
    };
    let mut vm: *mut JavaVM = std::ptr::null_mut();
    let mut env: *mut JNIEnv = std::ptr::null_mut();
    // SAFETY: the pointers are valid for the call.
    let made = unsafe {
        create(
            &mut vm,
            &mut env as *mut _ as *mut *mut c_void,
            &mut init as *mut _ as *mut c_void,
        )
    };
    if made != 0 {
        return Err(format!(
            "could not create the Java Virtual Machine ({made})"
        ));
    }
    // SAFETY: `env` is this thread's, valid until DestroyJavaVM.
    let code = unsafe { call(vm, env, &plan) };
    Ok(code)
}

macro_rules! jni {
    ($env:expr, $f:ident $(, $a:expr)*) => {
        ((**$env).$f.expect("JNI function"))($env $(, $a)*)
    };
}

/// Prints a pending exception's stack trace and clears it; whether there was one.
unsafe fn describe(env: *mut JNIEnv) -> bool {
    if jni!(env, ExceptionCheck) != 0 {
        jni!(env, ExceptionDescribe);
        jni!(env, ExceptionClear);
        return true;
    }
    false
}

unsafe fn call(vm: *mut JavaVM, env: *mut JNIEnv, plan: &Plan) -> i32 {
    let mut code;
    if plan.version {
        code = print_version(env);
    } else {
        let main = match (&plan.main, &plan.jar) {
            (Some(main), _) => Ok(main.clone()),
            (None, Some(jar)) => jar_main(jar),
            (None, None) => Err("no main class or -jar given".to_string()),
        };
        match main {
            Err(e) => {
                eprintln!("Error: {e}");
                code = 1;
            }
            Ok(main) => code = invoke(env, &main.replace('.', "/"), &plan.args),
        }
    }
    // Waits for the program's non-daemon threads, as the JDK launcher does.
    let destroyed = ((**vm).DestroyJavaVM.expect("JNI function"))(vm);
    if destroyed != 0 && code == 0 {
        code = 1;
    }
    code
}

unsafe fn invoke(env: *mut JNIEnv, main: &str, args: &[String]) -> i32 {
    let name = CString::new(main).unwrap_or_default();
    let class: jclass = jni!(env, FindClass, name.as_ptr());
    if class.is_null() {
        describe(env);
        eprintln!(
            "Error: Could not find or load main class {}",
            main.replace('/', ".")
        );
        return 1;
    }
    let method = jni!(
        env,
        GetStaticMethodID,
        class,
        c"main".as_ptr(),
        c"([Ljava/lang/String;)V".as_ptr()
    );
    if method.is_null() {
        describe(env);
        eprintln!(
            "Error: Main method not found in class {}",
            main.replace('/', ".")
        );
        return 1;
    }
    let string = jni!(env, FindClass, c"java/lang/String".as_ptr());
    let array = jni!(
        env,
        NewObjectArray,
        args.len() as jint,
        string,
        std::ptr::null_mut()
    );
    for (n, arg) in args.iter().enumerate() {
        let text = CString::new(arg.as_str()).unwrap_or_default();
        let value: jstring = jni!(env, NewStringUTF, text.as_ptr());
        jni!(
            env,
            SetObjectArrayElement,
            array,
            n as jint,
            value as jobject
        );
    }
    jni!(
        env,
        CallStaticVoidMethodA,
        class,
        method,
        [jni_sys::jvalue {
            l: array as jobject
        }]
        .as_ptr()
    );
    i32::from(describe(env))
}

/// `java -version`'s text, on stderr, from the VM's own properties.
unsafe fn print_version(env: *mut JNIEnv) -> i32 {
    let system = jni!(env, FindClass, c"java/lang/System".as_ptr());
    let get = jni!(
        env,
        GetStaticMethodID,
        system,
        c"getProperty".as_ptr(),
        c"(Ljava/lang/String;)Ljava/lang/String;".as_ptr()
    );
    let property = |key: &CStr| -> String {
        let key = jni!(env, NewStringUTF, key.as_ptr());
        let value = jni!(
            env,
            CallStaticObjectMethodA,
            system,
            get,
            [jni_sys::jvalue { l: key }].as_ptr()
        );
        if value.is_null() {
            return String::new();
        }
        let chars = jni!(
            env,
            GetStringUTFChars,
            value as jstring,
            std::ptr::null_mut()
        );
        let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
        jni!(env, ReleaseStringUTFChars, value as jstring, chars);
        text
    };
    let version = property(c"java.version");
    eprintln!(
        "openjdk version \"{version}\" {}",
        property(c"java.version.date")
    );
    eprintln!(
        "OpenJDK Runtime Environment (build {})",
        property(c"java.runtime.version")
    );
    eprintln!(
        "{} (build {}, mixed mode)",
        property(c"java.vm.name"),
        property(c"java.vm.version")
    );
    0
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let plan = match parse(&args) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(2);
        }
    };
    let stack = stack_size(&plan.options);
    // Like the JDK launcher, the VM and the main thread run on a thread of their own with a
    // stack of the size `-Xss` asks for, not on the process's primordial thread.
    let worker = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || run(plan));
    match worker.map(|w| w.join()) {
        Ok(Ok(Ok(code))) => ExitCode::from(code.clamp(0, 255) as u8),
        Ok(Ok(Err(e))) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        _ => {
            eprintln!("Error: the Java launcher thread failed");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_java_command_line_splits_into_vm_options_main_class_and_its_arguments() {
        let plan = parse(&args(&[
            "-XX:TieredStopAtLevel=1",
            "-Xmx512m",
            "-cp",
            "a.jar:b.jar",
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-Dx=1",
            "net.minecraft.data.Main",
            "--reports",
            "-Dnot=anoption",
        ]))
        .unwrap();
        assert_eq!(
            plan.options,
            [
                "-XX:TieredStopAtLevel=1",
                "-Xmx512m",
                "--add-opens=java.base/java.lang=ALL-UNNAMED",
                "-Dx=1"
            ]
        );
        assert_eq!(plan.class_path.as_deref(), Some("a.jar:b.jar"));
        assert_eq!(plan.main.as_deref(), Some("net.minecraft.data.Main"));
        assert_eq!(plan.args, ["--reports", "-Dnot=anoption"]);
    }

    #[test]
    fn jar_takes_the_rest_as_program_arguments() {
        let plan = parse(&args(&["-Xmx1g", "-jar", "server.jar", "--reports", "-x"])).unwrap();
        assert_eq!(plan.jar.as_deref(), Some("server.jar"));
        assert_eq!(plan.args, ["--reports", "-x"]);
        assert!(plan.main.is_none());
        assert!(parse(&args(&["-jar"])).is_err());
    }

    #[test]
    fn the_main_thread_stack_follows_xss() {
        assert_eq!(stack_size(&args(&["-Xss16m"])), 16 << 20);
        assert_eq!(stack_size(&args(&["-Xss512k", "-Xss2m"])), 2 << 20);
        assert_eq!(stack_size(&[]), 8 << 20);
    }
}
