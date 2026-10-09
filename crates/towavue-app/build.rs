#[path = "build/ffmpeg.rs"]
mod ffmpeg;

fn run_hidden(command: &mut std::process::Command, operation: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().expect(operation);
    assert!(
        output.status.success(),
        "{operation}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        ffmpeg::configure();
        println!("cargo:rerun-if-changed=assets/windows.rc");
        for icon in ["towavue", "image", "video", "audio"] {
            println!("cargo:rerun-if-changed=assets/{icon}.ico");
        }
        println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
        let sdk = find_msvc_tools::find_windows_sdk(std::env::consts::ARCH)
            .expect("Windows SDK is required to compile the application icon");
        let compiler = sdk
            .path()
            .map(|path| path.join("rc.exe"))
            .find(|path| path.is_file())
            .expect("Windows SDK resource compiler rc.exe");
        let resource = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output"))
            .join("towavue.res");
        let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo version");
        let components: Vec<u16> = version
            .split('.')
            .map(|part| part.parse().expect("stable Windows version component"))
            .collect();
        assert_eq!(
            components.len(),
            3,
            "release versions have three components"
        );
        let numbers = format!("{},{},{},0", components[0], components[1], components[2]);
        let source = std::fs::read_to_string("assets/windows.rc")
            .expect("resource template")
            .replace("@VERSION_COMPONENTS@", &numbers)
            .replace("@VERSION@", &version);
        let generated = resource.with_extension("rc");
        std::fs::write(&generated, source).expect("versioned resource");
        let mut command = std::process::Command::new(compiler);
        command
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg(&generated);
        run_hidden(&mut command, "compile application icon resource");
        // Include test hosts so resource-loading checks exercise the same asset.
        println!("cargo:rustc-link-arg={}", resource.display());
        // TaskDialogIndirect requires Common Controls v6 in executables and test hosts.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
