use super::*;
use crate::avif_container::{self as container, boxes, one};
use ffmpeg::format::Pixel;
use ffmpeg_next as ffmpeg;

impl From<container::Error> for ImageDecodeError {
    fn from(error: container::Error) -> Self {
        match error {
            container::Error::Cancelled => Self::Cancelled,
            container::Error::Io(error) => Self::Open(error),
            container::Error::Invalid(message) => Self::Avif(message),
        }
    }
}

fn invalid(message: &str) -> ImageDecodeError {
    ImageDecodeError::Avif(message.into())
}

fn ffmpeg_error(error: ffmpeg::Error) -> ImageDecodeError {
    ImageDecodeError::Ffmpeg(error.into())
}

pub(super) fn decode(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
    preview: &mut ImagePreviewCallback<'_>,
    first_only: bool,
) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    let (color, alpha, premultiplied) = select(path, current)?;
    ffmpeg::init().map_err(ffmpeg_error)?;
    let mut color_decoder = PlaneDecoder::new(path, &color, Pixel::RGBA, byte_limit)?;
    let mut alpha_decoder = alpha
        .map(|alpha| PlaneDecoder::new(path, &alpha, Pixel::GRAY8, byte_limit))
        .transpose()?;
    let mut frames: Vec<DecodedImageFrame> = Vec::new();
    let mut remaining = byte_limit;
    let mut previous_time = None;
    while let Some(mut color_frame) = color_decoder.next(current)? {
        check_current(current)?;
        remaining = remaining
            .checked_sub(color_frame.pixels.len())
            .ok_or(ImageDecodeError::TooLarge)?;
        if frames.len() == 65536 {
            return Err(ImageDecodeError::TooLarge);
        }
        if let Some(alpha_decoder) = &mut alpha_decoder {
            let alpha = alpha_decoder
                .next(current)?
                .ok_or_else(|| invalid("missing alpha frame"))?;
            if color_frame.size != alpha.size
                || color_frame.time != alpha.time
                || color_frame.duration != alpha.duration
            {
                return Err(invalid("alpha geometry or timing differs from color"));
            }
            // Older libavif files omit alpha transforms. An explicit transform
            // must agree; rotate the merged canvas once, never just its colors.
            if alpha.orientation.is_some_and(|orientation| {
                orientation != color_frame.orientation.unwrap_or_default()
            }) {
                return Err(invalid("alpha orientation differs from color"));
            }
            if alpha
                .aperture
                .is_some_and(|aperture| aperture != color_frame.aperture.unwrap_or_default())
            {
                return Err(invalid("alpha clean aperture differs from color"));
            }
            for (pixel, alpha) in color_frame
                .pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(alpha.pixels)
            {
                pixel[3] = alpha;
                if premultiplied {
                    for channel in &mut pixel[..3] {
                        *channel = if alpha == 0 {
                            0
                        } else {
                            ((u32::from(*channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha))
                                .min(255) as u8
                        };
                    }
                }
            }
        }
        if let Some(aperture) = color_frame.aperture {
            let crop = aperture.rectangle(color_frame.size)?;
            let stride = color_frame.size.0 as usize * 4;
            let row_bytes = crop.width as usize * 4;
            // Compact merged RGBA in place, before rotation and before any preview.
            for y in 0..crop.height as usize {
                check_current(current)?;
                let start = (crop.y as usize + y) * stride + crop.x as usize * 4;
                color_frame
                    .pixels
                    .copy_within(start..start + row_bytes, y * row_bytes);
            }
            color_frame
                .pixels
                .truncate(row_bytes * crop.height as usize);
            color_frame.size = (crop.width, crop.height);
        }
        if let Some(orientation) = color_frame.orientation
            && orientation != crate::VideoOrientation::default()
        {
            check_current(current)?;
            let image = image::RgbaImage::from_raw(
                color_frame.size.0,
                color_frame.size.1,
                color_frame.pixels,
            )
            .ok_or_else(|| invalid("invalid decoded RGBA canvas"))?;
            let mut image = image::DynamicImage::ImageRgba8(image);
            image.apply_orientation(orientation.image_orientation());
            let image = image.into_rgba8();
            color_frame.size = image.dimensions();
            color_frame.pixels = image.into_raw();
            check_current(current)?;
        }
        if let Some(previous) = previous_time {
            if color_frame.time <= previous {
                return Err(invalid("non-increasing frame time"));
            }
            frames.last_mut().expect("previous frame").delay = delay(color_frame.time - previous);
        }
        previous_time = Some(color_frame.time);
        let frame = DecodedImageFrame {
            width: color_frame.size.0,
            height: color_frame.size.1,
            rgba: color_frame.pixels,
            delay: delay(color_frame.duration.unwrap_or(100_000_000)),
        };
        if frames.is_empty() {
            preview(frame.width, frame.height, &frame.rgba);
        }
        check_current(current)?;
        frames.push(frame);
        if first_only {
            return Ok(frames);
        }
    }
    if let Some(alpha) = &mut alpha_decoder
        && alpha.next(current)?.is_some()
    {
        return Err(invalid("extra alpha frames"));
    }
    Ok(frames)
}

#[derive(Clone, Copy)]
struct Selection {
    id: u32,
    timing: Option<(u32, u64)>,
}

fn select(
    path: &Path,
    current: &dyn Fn() -> bool,
) -> Result<(Selection, Option<Selection>, bool), ImageDecodeError> {
    check_current(current)?;
    let mut file = File::open(path).map_err(ImageDecodeError::Open)?;
    let length = file.metadata().map_err(ImageDecodeError::Open)?.len();
    let root = boxes(&mut file, 0, length, current)?;
    let Some(movie) = one(&root, b"moov")? else {
        let still = container::still::read(&mut file, &root, current)?;
        return Ok((
            Selection {
                id: still.color,
                timing: None,
            },
            still.alpha.map(|id| Selection { id, timing: None }),
            still.premultiplied,
        ));
    };
    let tracks = boxes(&mut file, movie.start, movie.end, current)?
        .into_iter()
        .filter(|item| &item.kind == b"trak")
        .map(|item| container::track(&mut file, item, current, false))
        .collect::<Result<Vec<_>, _>>()?;
    let colors: Vec<_> = tracks
        .iter()
        .filter(|track| track.alpha_for.is_none())
        .collect();
    if colors.len() != 1 || tracks.len() > 2 {
        return Err(invalid("ambiguous color or auxiliary tracks"));
    }
    let color = colors[0];
    let alpha = tracks
        .iter()
        .find(|track| track.alpha_for == Some(color.id));
    if tracks.len() == 2 && alpha.is_none() {
        return Err(invalid("unlinked auxiliary track"));
    }
    if color
        .premultiplied_with
        .is_some_and(|id| alpha.is_none_or(|alpha| alpha.id != id))
    {
        return Err(invalid("invalid premultiplied-alpha reference"));
    }

    let selected = |track: &container::Track| Selection {
        id: track.id,
        timing: Some((track.timescale, track.duration)),
    };
    Ok((
        selected(color),
        alpha.map(selected),
        color.premultiplied_with.is_some(),
    ))
}

fn delay(nanos: i128) -> Duration {
    Duration::from_nanos(nanos.clamp(10_000_000, i128::from(u64::MAX)) as u64)
}

struct PlaneFrame {
    size: (u32, u32),
    time: i128,
    duration: Option<i128>,
    pixels: Vec<u8>,
    orientation: Option<crate::VideoOrientation>,
    aperture: Option<container::CleanAperture>,
}

// Independent demux cursors permit interleaved or contiguous alpha data without
// retaining an entire color/alpha sequence while waiting for its matching track.
struct PlaneDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::codec::decoder::Video,
    index: usize,
    time_base: ffmpeg::Rational,
    pixel: Pixel,
    scaler: Option<ffmpeg::software::scaling::Context>,
    eof: bool,
    byte_limit: usize,
    orientation: Option<crate::VideoOrientation>,
    aperture: Option<container::CleanAperture>,
}

impl PlaneDecoder {
    fn new(
        path: &Path,
        track: &Selection,
        pixel: Pixel,
        byte_limit: usize,
    ) -> Result<Self, ImageDecodeError> {
        // AVIF holds may use the full unsigned stts range, unlike legacy MOV
        // files that encode negative DTS corrections in the same field.
        let mut options = ffmpeg::Dictionary::new();
        options.set("max_stts_delta", &u32::MAX.to_string());
        options.set("err_detect", "explode");
        let input = ffmpeg::format::input_with_dictionary(path, options).map_err(ffmpeg_error)?;
        let mut matching = input.streams().filter(|stream| {
            stream.id() as u32 == track.id
                && track.timing.is_none_or(|(timescale, duration)| {
                    stream.time_base() == ffmpeg::Rational(1, timescale as i32)
                        && stream.duration() > 0
                        && stream.duration() as u64 == duration
                })
        });
        let stream = matching
            .next()
            .ok_or_else(|| invalid("missing image item or timed track"))?;
        if matching.next().is_some() || stream.parameters().id() != ffmpeg::codec::Id::AV1 {
            return Err(invalid("ambiguous or non-AV1 image item/track"));
        }
        let index = stream.index();
        let matrix = stream
            .side_data()
            .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix);
        let orientation = matrix
            .as_ref()
            .map(|data| crate::VideoOrientation::from_bytes(Some(data.data())))
            .transpose()
            .map_err(ImageDecodeError::Ffmpeg)?;
        let time_base = stream.time_base();
        let aperture = stream
            .side_data()
            .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::FRAME_CROPPING)
            .map(|data| container::CleanAperture::from_bytes(data.data()))
            .transpose()?;
        if time_base.numerator() <= 0 || time_base.denominator() <= 0 {
            return Err(invalid("invalid image time base"));
        }
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(ffmpeg_error)?;
        let mut options = ffmpeg::Dictionary::new();
        options.set("max_pixels", &(IMAGE_BYTE_LIMIT / 4).to_string());
        // Avoid frame-thread lookahead delaying a first-only preview; retain
        // decoder-selected worker parallelism within a frame.
        options.set("flags", "+low_delay");
        options.set("err_detect", "explode");
        let codec = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::AV1)
            .ok_or_else(|| invalid("AV1 decoder unavailable"))?;
        let decoder = context
            .decoder()
            .open_as_with(codec, options)
            .and_then(|opened| opened.video())
            .map_err(ffmpeg_error)?;
        check_size((decoder.width(), decoder.height()), byte_limit)?;
        Ok(Self {
            input,
            decoder,
            index,
            time_base,
            pixel,
            scaler: None,
            eof: false,
            byte_limit,
            orientation,
            aperture,
        })
    }

    fn next(&mut self, current: &dyn Fn() -> bool) -> Result<Option<PlaneFrame>, ImageDecodeError> {
        loop {
            check_current(current)?;
            let mut frame = ffmpeg::frame::Video::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    let size = (frame.width(), frame.height());
                    check_size(size, self.byte_limit)?;
                    if self.scaler.as_ref().is_none_or(|scaler| {
                        scaler.input().format != frame.format()
                            || scaler.input().width != size.0
                            || scaler.input().height != size.1
                    }) {
                        self.scaler = Some(
                            ffmpeg::software::scaling::Context::get(
                                frame.format(),
                                size.0,
                                size.1,
                                self.pixel,
                                size.0,
                                size.1,
                                ffmpeg::software::scaling::flag::Flags::BILINEAR,
                            )
                            .map_err(ffmpeg_error)?,
                        );
                    }
                    let mut converted = ffmpeg::frame::Video::empty();
                    self.scaler
                        .as_mut()
                        .expect("scaler")
                        .run(&frame, &mut converted)
                        .map_err(ffmpeg_error)?;
                    let row_bytes = size.0 as usize * if self.pixel == Pixel::RGBA { 4 } else { 1 };
                    let mut pixels = Vec::with_capacity(row_bytes * size.1 as usize);
                    for row in converted
                        .data(0)
                        .chunks_exact(converted.stride(0))
                        .take(size.1 as usize)
                    {
                        pixels.extend_from_slice(&row[..row_bytes]);
                    }
                    let time = frame
                        .timestamp()
                        .ok_or_else(|| invalid("missing presentation time"))?;
                    let nanos = |time: i64| {
                        i128::from(time) * i128::from(self.time_base.numerator()) * 1_000_000_000
                            / i128::from(self.time_base.denominator())
                    };
                    return Ok(Some(PlaneFrame {
                        aperture: self.aperture,
                        orientation: frame
                            .side_data(ffmpeg::util::frame::side_data::Type::DisplayMatrix)
                            .map(|data| crate::VideoOrientation::from_bytes(Some(data.data())))
                            .transpose()
                            .map_err(ImageDecodeError::Ffmpeg)?
                            .or(self.orientation),
                        size,
                        time: nanos(time),
                        duration: (frame.packet().duration > 0)
                            .then(|| nanos(frame.packet().duration)),
                        pixels,
                    }));
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno })
                    if errno == ffmpeg::error::EAGAIN && !self.eof => {}
                Err(error) => return Err(ffmpeg_error(error)),
            }
            loop {
                check_current(current)?;
                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() == self.index => {
                        if packet.is_corrupt() {
                            return Err(invalid("corrupt sample"));
                        }
                        self.decoder.send_packet(&packet).map_err(ffmpeg_error)?;
                        break;
                    }
                    Ok(()) => {}
                    Err(ffmpeg::Error::Eof) => {
                        self.eof = true;
                        self.decoder.send_eof().map_err(ffmpeg_error)?;
                        break;
                    }
                    Err(error) => return Err(ffmpeg_error(error)),
                }
            }
        }
    }
}

fn check_size(size: (u32, u32), byte_limit: usize) -> Result<(), ImageDecodeError> {
    if size.0 == 0
        || size.1 == 0
        || u64::from(size.0) * u64::from(size.1) > (byte_limit.min(IMAGE_BYTE_LIMIT) / 4) as u64
    {
        Err(ImageDecodeError::TooLarge)
    } else {
        Ok(())
    }
}
