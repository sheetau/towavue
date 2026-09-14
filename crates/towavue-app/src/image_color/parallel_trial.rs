//! Verification-only initialized-row controls; not compiled into the application.

pub(crate) fn color_image(
    frame: &towavue_runtime_windows::DecodedImageFrame,
    parallel: bool,
) -> egui::ColorImage {
    let size = [frame.width as usize, frame.height as usize];
    assert_eq!(size[0] * size[1] * 4, frame.rgba.len());
    // Safe disjoint output slices require initialized storage. Include
    // that cost, thread creation and joining in the measured conversion.
    let mut pixels = vec![egui::Color32::TRANSPARENT; size[0] * size[1]];
    let fill = |output: &mut [egui::Color32], rgba: &[u8]| {
        for (out, row) in output
            .chunks_mut(size[0].max(1))
            .zip(rgba.chunks_exact(size[0].max(1) * 4))
        {
            let row = row.as_chunks::<4>().0;
            let mask = u32::from_ne_bytes([0, 0, 0, 255]);
            let opaque = row.chunks(32).all(|block| {
                block
                    .iter()
                    .fold(u32::MAX, |bits, p| bits & u32::from_ne_bytes(*p))
                    & mask
                    == mask
            });
            for (out, p) in out.iter_mut().zip(row) {
                *out = if opaque {
                    egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
                } else {
                    egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
                };
            }
        }
    };
    if parallel {
        let split = size[0] * size[1].div_ceil(2);
        let (first, second) = pixels.split_at_mut(split);
        let (first_rgba, second_rgba) = frame.rgba.split_at(split * 4);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| fill(first, first_rgba));
            fill(second, second_rgba);
            worker.join().expect("color worker");
        });
    } else {
        fill(&mut pixels, &frame.rgba);
    }
    egui::ColorImage::new(size, pixels)
}
