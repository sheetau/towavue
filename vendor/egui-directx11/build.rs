use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::ID3DInclude;
use windows::core::{PCSTR, s};

fn main() {
    println!("cargo:rerun-if-changed=shaders/egui.hlsl");
    println!("cargo:rerun-if-changed=shaders/mipmap.hlsl");
    for (source, entry, target, output) in [
        (
            include_bytes!("shaders/egui.hlsl").as_slice(),
            s!("vs_egui"),
            s!("vs_5_0"),
            "egui-vertex.cso",
        ),
        (
            include_bytes!("shaders/egui.hlsl").as_slice(),
            s!("ps_egui"),
            s!("ps_5_0"),
            "egui-pixel.cso",
        ),
        (
            include_bytes!("shaders/egui.hlsl").as_slice(),
            s!("ps_egui_minification"),
            s!("ps_5_0"),
            "egui-minification.cso",
        ),
        (
            include_bytes!("shaders/mipmap.hlsl").as_slice(),
            s!("vs_mipmap"),
            s!("vs_5_0"),
            "mipmap-vertex.cso",
        ),
        (
            include_bytes!("shaders/mipmap.hlsl").as_slice(),
            s!("ps_mipmap"),
            s!("ps_5_0"),
            "mipmap-pixel.cso",
        ),
    ] {
        let mut bytecode = None;
        // Source and strings remain valid throughout compilation. The owned blob
        // outlives the borrowed slice written into Cargo's build output.
        unsafe {
            D3DCompile(
                source.as_ptr().cast(),
                source.len(),
                PCSTR::null(),
                None,
                None::<&ID3DInclude>,
                entry,
                target,
                D3DCOMPILE_OPTIMIZATION_LEVEL3,
                0,
                &mut bytecode,
                None,
            )
            .expect("compile fixed shader");
            let bytecode = bytecode.expect("successful shader compilation");
            let bytes = std::slice::from_raw_parts(
                bytecode.GetBufferPointer().cast::<u8>(),
                bytecode.GetBufferSize(),
            );
            let directory = std::env::var_os("OUT_DIR").expect("Cargo build output");
            std::fs::write(std::path::PathBuf::from(directory).join(output), bytes)
                .expect("write fixed shader bytecode");
        }
    }
}
