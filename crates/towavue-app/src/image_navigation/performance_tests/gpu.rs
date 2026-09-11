use super::*;
use towavue_runtime_windows::VerificationMemory;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "generates 100 large JPEGs; requires FFMPEG_DIR, hardware D3D11 and a Release test build"]
fn hundred_large_images_report_gpu_navigation_and_memory() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::gpu::hundred_large_images_report_gpu_navigation_and_memory",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            match FrameRenderer::new(&window) {
                Ok(renderer) => measure(self.root.clone(), Some(renderer)),
                Err(error) => eprintln!("SKIP NAV100 GPU: hardware D3D11 unavailable: {error}"),
            }
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { root })
        .expect("GPU measurement");
}

pub(super) fn submit<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &Application<N>,
    context: &egui::Context,
    output: egui::FullOutput,
    renderer: &mut FrameRenderer,
    verify: bool,
) -> Duration {
    let density = output.pixels_per_point;
    let (width, height) = ((960.0 * density) as u32, (576.0 * density) as u32);
    let image = app
        .image
        .as_ref()
        .or_else(|| app.image_handoff.as_ref().map(|held| &held.image));
    let bounds = image.and_then(|image| {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == image.texture.id() => {
                Some(mesh.calc_bounds() * density)
            }
            _ => None,
        })
    });
    let started = Instant::now();
    renderer
        .resize_surface(width, height)
        .expect("surface size");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    renderer.render_ui(context, output).expect("GPU UI");
    if verify && let (Some(image), Some(bounds)) = (image, bounds) {
        let pixels = renderer
            .verification_surface_rgba()
            .expect("pixel readback");
        let frame = &image.decoded.frames[0];
        for row in 0..8 {
            for column in 0..8 {
                let x =
                    (bounds.left() + bounds.width() * (column as f32 + 0.5) / 8.0).floor() as usize;
                let y =
                    (bounds.top() + bounds.height() * (row as f32 + 0.5) / 8.0).floor() as usize;
                assert!(x < width as usize && y < height as usize);
                let sx = (((x as f32 + 0.5 - bounds.left()) / bounds.width()) * frame.width as f32)
                    .floor() as usize;
                let sy = (((y as f32 + 0.5 - bounds.top()) / bounds.height()) * frame.height as f32)
                    .floor() as usize;
                let source = (sy * frame.width as usize + sx) * 4;
                let target = (y * width as usize + x) * 4;
                assert!(
                    pixels[target..target + 4]
                        .iter()
                        .zip(&frame.rgba[source..source + 4])
                        .all(|(actual, expected)| actual.abs_diff(*expected) <= 2),
                    "GPU sample ({x},{y}) differs from original ({sx},{sy}): {:?} vs {:?}",
                    &pixels[target..target + 4],
                    &frame.rgba[source..source + 4]
                );
            }
        }
    }
    renderer.present_surface().expect("Present");
    started.elapsed()
}

pub(super) struct Memory {
    start: VerificationMemory,
    end: VerificationMemory,
    gpu_max: Option<[u64; 2]>,
}

impl Memory {
    pub fn new(renderer: &FrameRenderer) -> Self {
        let start = renderer.verification_memory().expect("process memory");
        if let Err(error) = &start.gpu_local_nonlocal {
            eprintln!("SKIP NAV100 GPU memory counters: {error}");
        }
        Self {
            end: renderer.verification_memory().expect("process memory"),
            gpu_max: start.gpu_local_nonlocal.as_ref().ok().copied(),
            start,
        }
    }
    pub fn sample(&mut self, renderer: &FrameRenderer) {
        self.end = renderer.verification_memory().expect("process memory");
        match (&mut self.gpu_max, &self.end.gpu_local_nonlocal) {
            (Some(maximum), Ok(usage)) => {
                for i in 0..2 {
                    maximum[i] = maximum[i].max(usage[i]);
                }
            }
            (Some(_), Err(error)) => {
                eprintln!("SKIP NAV100 GPU memory counters: {error}");
                self.gpu_max = None;
            }
            _ => {}
        }
    }
    pub fn report(&self) {
        let mib = |bytes: u64| bytes as f64 / 1048576.0;
        eprintln!(
            "NAV100_MEMORY working_start_mib={:.1} working_end_mib={:.1} private_start_mib={:.1} private_end_mib={:.1} process_peak_working_mib={:.1} process_peak_commit_mib={:.1} gpu_sampled_max_local_nonlocal_mib={:?}",
            mib(self.start.working_set),
            mib(self.end.working_set),
            mib(self.start.private_bytes),
            mib(self.end.private_bytes),
            mib(self.end.peak_working_set),
            mib(self.end.peak_commit),
            self.gpu_max.map(|usage| usage.map(mib))
        );
    }
}
