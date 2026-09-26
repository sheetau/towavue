//! Recognize millisecond AAC timestamps retained in a finer container time base.
use ffmpeg_next::{Stream, codec};

pub(crate) fn quantized_aac(stream: &Stream<'_>) -> bool {
    let parameters = stream.parameters();
    if parameters.id() != codec::Id::AAC {
        return false;
    }
    let base = f64::from(stream.time_base());
    if !(0.0..0.001).contains(&base) {
        return false;
    }
    // The demuxer owns both codec parameters and index entries. Read bounded
    // scalar metadata while the stream is immutably borrowed; retain no pointer,
    // perform no seek/demux I/O and never modify FFmpeg's index. The entry getter
    // takes AVStream* but only bounds-checks and returns an existing const entry.
    unsafe {
        let rate = (*parameters.as_ptr()).sample_rate;
        if rate <= 0 {
            return false;
        }
        let count = ffmpeg_next::ffi::avformat_index_get_entries_count(stream.as_ptr());
        if count < 10 {
            return false;
        }
        let mut previous = None;
        let mut minimum = f64::INFINITY;
        let mut maximum = 0.0_f64;
        let mut regular = 0;
        // Skip priming/first-packet metadata. Ordinary exact AAC timestamps fail
        // the millisecond-grid test; isolated small real gaps must not enable it.
        for index in 2..count.min(34) {
            let Some(entry) =
                ffmpeg_next::ffi::avformat_index_get_entry(stream.as_ptr().cast_mut(), index)
                    .as_ref()
            else {
                return false;
            };
            if let Some(previous) = previous {
                let Some(delta) = entry.timestamp.checked_sub(previous) else {
                    return false;
                };
                let seconds = delta as f64 * base;
                if seconds <= 0.0
                    || (seconds * 1000.0 - (seconds * 1000.0).round()).abs() > base * 510.0
                {
                    return false;
                }
                if (seconds - 1024.0 / f64::from(rate)).abs() <= 0.001 + base {
                    minimum = minimum.min(seconds);
                    maximum = maximum.max(seconds);
                    regular += 1;
                }
            }
            previous = Some(entry.timestamp);
        }
        regular >= 7 && maximum - minimum > base
    }
}
