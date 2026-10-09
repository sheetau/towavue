use std::path::{Path, PathBuf};

const DELAYED_LIBRARIES: [&str; 5] = ["avcodec", "avfilter", "avformat", "swresample", "swscale"];

pub fn configure() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let prefix =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("set FFMPEG_DIR to the FFmpeg prefix"));
    let bin = prefix.join("bin");
    let lib = if prefix.join("lib/amd64").is_dir() {
        prefix.join("lib/amd64")
    } else {
        prefix.join("lib")
    };
    println!("cargo:rerun-if-changed={}", bin.display());
    println!("cargo:rerun-if-changed={}", lib.display());
    let output =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output")).join("ffmpeg-imports");
    std::fs::create_dir_all(&output).expect("create FFmpeg import-library output");
    let target = std::env::var("TARGET").expect("Cargo target");
    assert_eq!(
        std::env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
        Ok("x86_64")
    );
    let librarian = find_msvc_tools::find_tool(&target, "lib.exe")
        .expect("Visual Studio librarian is required for FFmpeg delay imports");
    let mut dlls = Vec::new();
    for library in DELAYED_LIBRARIES {
        let dll = dll_name(&bin, library);
        let definition_name = dll.trim_end_matches(".dll").to_owned() + ".def";
        let definition = [lib.join(&definition_name), bin.join(&definition_name)]
            .into_iter()
            .find(|path| path.is_file());
        let destination = output.join(format!("{library}.lib"));
        if let Some(definition) = definition {
            // GNU-style import archives can silently bypass MSVC delay imports.
            // Reuse the prefix's exports, without rebuilding or changing its DLL.
            let mut command = librarian.to_command();
            command
                .arg("/nologo")
                .arg("/machine:x64")
                .arg(format!("/def:{}", definition.display()))
                .arg(format!("/name:{dll}"))
                .arg(format!("/out:{}", destination.display()));
            super::run_hidden(&mut command, "create FFmpeg delay import library");
        } else {
            // Native MSVC prefixes already contain suitable import libraries.
            // LNK4199 is fatal below if an incompatible archive slips through.
            std::fs::copy(lib.join(format!("{library}.lib")), &destination)
                .expect("copy native FFmpeg import library (or supply its versioned .def)");
        }
        println!("cargo:rustc-link-arg-bin=towavue=/DELAYLOAD:{dll}");
        dlls.push(dll);
    }
    // avutil stays eager: diagnostics installs its callback before codec work.
    // No delay-unload flag: resolved addresses remain valid until process exit.
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-arg-bin=towavue=delayimp.lib");
    println!("cargo:rustc-link-arg-bin=towavue=/WX:4199");
    println!(
        "cargo:rustc-env=TOWAVUE_DELAYED_FFMPEG_DLLS={}",
        dlls.join(";")
    );
}

fn dll_name(bin: &Path, library: &str) -> String {
    let prefix = format!("{library}-");
    let names: Vec<_> = std::fs::read_dir(bin)
        .expect("read FFmpeg DLL directory")
        .map(|entry| entry.expect("read FFmpeg DLL entry"))
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.strip_prefix(&prefix)
                .and_then(|tail| tail.strip_suffix(".dll"))
                .is_some_and(|version| {
                    !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
        .collect();
    assert_eq!(
        names.len(),
        1,
        "expected one {library}-<major>.dll in FFMPEG_DIR/bin"
    );
    names.into_iter().next().expect("one DLL name")
}
