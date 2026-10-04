//! Read-only split of the ordinary FFmpeg input open and stream probe.
//! Reports internal costs and header changes, not end-to-end playback latency.
use ffmpeg_next as ffmpeg;
use std::{error::Error, ffi::CString, path::PathBuf, ptr, time::Instant};

fn snapshot(input: &ffmpeg::format::context::Input) -> String {
    let mut result = format!(
        "format={} duration={} streams={}",
        input.format().name(),
        input.duration(),
        input.nb_streams()
    );
    for stream in input.streams() {
        let parameters = stream.parameters();
        // SAFETY: parameters is borrowed from the live, exclusively owned input.
        let p = unsafe { &*parameters.as_ptr() };
        result.push_str(&format!(
            " [index={} codec={:?} format={} rate={} channels={} bits={} block={} frame={} padding={} extra={} duration={} start={} time_base={}]",
            stream.index(), parameters.id(), p.format, p.sample_rate,
            p.ch_layout.nb_channels, p.bits_per_coded_sample, p.block_align,
            p.frame_size, p.initial_padding, p.extradata_size,
            stream.duration(), stream.start_time(), stream.time_base()
        ));
    }
    result
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use --release".into());
    }
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("explicit input path required")?,
    );
    let repeats: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1".into())
        .parse()?;
    if !(1..=1000).contains(&repeats) || !path.is_file() {
        return Err("expected existing regular file and 1..=1000 repetitions".into());
    }
    let stamp = || -> std::io::Result<_> {
        let metadata = path.metadata()?;
        Ok((metadata.len(), metadata.modified()?))
    };
    let before = stamp()?;
    let name = CString::new(path.to_str().ok_or("input path is not UTF-8")?)?;
    ffmpeg::init()?;
    for index in 0..repeats {
        let started = Instant::now();
        // SAFETY: open_input initializes and owns the context, freeing it on
        // failure. On success transfer it once into the RAII Input. The path
        // outlives the call; no custom IO, format, options or callbacks are used.
        let mut input = unsafe {
            let mut raw = ptr::null_mut();
            let result = ffmpeg::ffi::avformat_open_input(
                &mut raw,
                name.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
            );
            if result < 0 {
                return Err(ffmpeg::Error::from(result).into());
            }
            ffmpeg::format::context::Input::wrap(raw)
        };
        let opened = started.elapsed().as_secs_f64() * 1000.0;
        let header = snapshot(&input);
        let started = Instant::now();
        // SAFETY: this input is exclusively owned. Drop closes it on success
        // and failure. Match format::input's ordinary default stream probing.
        let result =
            unsafe { ffmpeg::ffi::avformat_find_stream_info(input.as_mut_ptr(), ptr::null_mut()) };
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        let probed = started.elapsed().as_secs_f64() * 1000.0;
        let complete = snapshot(&input);
        let started = Instant::now();
        drop(input);
        let closed = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "INPUT_COST index={index} open_ms={opened:.6} probe_ms={probed:.6} close_ms={closed:.6}"
        );
        if index == 0 {
            println!("HEADER {header}\nPROBED {complete}");
        }
    }
    if stamp()? != before {
        return Err("input changed during measurement".into());
    }
    Ok(())
}
