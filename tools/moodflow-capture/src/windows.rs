//! OS APIs stay in this worker; the Hub supervises its lifetime and reviews the result.
use anyhow::{Result, ensure};
use image::{ImageBuffer, RgbaImage};
use moodflow_capsule::capture::{self, MAX_ASSET_BYTES, Request, Target};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

mod audio;
mod privacy;
#[cfg(test)]
mod tests;

pub fn run() -> Result<Value> {
    unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )?;
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let mut reader = BufReader::new(std::io::stdin());
    let mut line = String::new();
    Read::by_ref(&mut reader).take(65537).read_line(&mut line)?;
    ensure!(line.len() <= 65536, "capture.invalid_request");
    let request: Request = serde_json::from_str(&line)?;
    request.validate()?;
    ensure!(privacy::allowed(), "capture.privacy");
    if request.operation == "enumerate" {
        return targets();
    }
    if request.operation == "inspect_target" {
        let Target::Window { hwnd, pid } = request
            .target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("capture.invalid_target"))?
        else {
            anyhow::bail!("capture.invalid_target")
        };
        let window = Window::from_raw_hwnd(usize::try_from(*hwnd)? as *mut std::ffi::c_void);
        ensure!(
            window.is_valid() && window.process_id()? == *pid,
            "capture.window_changed"
        );
        return Ok(json!({"process":window.process_name()?}));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let input_stop = stop.clone();
    std::thread::spawn(move || {
        let mut command = String::new();
        loop {
            command.clear();
            if Read::by_ref(&mut reader)
                .take(16)
                .read_line(&mut command)
                .unwrap_or(0)
                == 0
                || command.trim() == "stop"
            {
                input_stop.store(true, Ordering::Release);
                break;
            }
        }
    });
    // Reserve only a fresh output. A malformed request must never delete an existing file.
    if let Some(path) = &request.output {
        fs::File::create_new(path)?;
    }
    let result = execute(&request, &stop);
    if result.is_err()
        && let Some(path) = &request.output
    {
        let _ = fs::remove_file(path);
    }
    result
}
fn execute(request: &Request, stop: &Arc<AtomicBool>) -> Result<Value> {
    let start = Instant::now();
    while start.elapsed().as_millis() < request.delay_ms as u128 {
        check(stop)?;
        std::thread::sleep(Duration::from_millis(50));
    }
    match request.operation.as_str() {
        "inspect" | "dib" => {
            use std::io::Write;
            let image = open_image(
                request
                    .input
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("capture.invalid_input"))?,
            )?
            .to_rgba8();
            check(stop)?;
            if request.operation == "dib" {
                let mut output = fs::OpenOptions::new().write(true).truncate(true).open(
                    request
                        .output
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("capture.invalid_output"))?,
                )?;
                let mut header = Vec::with_capacity(40);
                header.extend_from_slice(&40_u32.to_le_bytes());
                header.extend_from_slice(&(image.width() as i32).to_le_bytes());
                header.extend_from_slice(&(-(image.height() as i32)).to_le_bytes());
                header.extend_from_slice(&1_u16.to_le_bytes());
                header.extend_from_slice(&32_u16.to_le_bytes());
                header.extend_from_slice(&0_u32.to_le_bytes());
                header.extend_from_slice(&(image.width() * image.height() * 4).to_le_bytes());
                header.extend_from_slice(&[0; 16]);
                output.write_all(&header)?;
                let mut row = Vec::with_capacity(image.width() as usize * 4);
                for pixels in image.rows() {
                    row.clear();
                    for pixel in pixels {
                        let flatten = |v: u8| {
                            ((u16::from(v) * u16::from(pixel[3])
                                + 255 * (255 - u16::from(pixel[3])))
                                / 255) as u8
                        };
                        row.extend_from_slice(&[
                            flatten(pixel[2]),
                            flatten(pixel[1]),
                            flatten(pixel[0]),
                            255,
                        ]);
                    }
                    output.write_all(&row)?;
                }
                output.sync_all()?;
            }
            Ok(json!({"width":image.width(),"height":image.height(),"duration_ms":0}))
        }
        "screenshot" => {
            let image = screenshot(request, stop)?;
            check(stop)?;
            image.save_with_format(
                request
                    .output
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("capture.invalid_output"))?,
                image::ImageFormat::Png,
            )?;
            Ok(json!({"width":image.width(),"height":image.height(),"duration_ms":0}))
        }
        "record" => record(request, stop),
        "scroll" => scroll(request, stop),
        "ocr" => ocr(
            request
                .input
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_input"))?,
            stop,
        ),
        "qr" => qr(
            request
                .input
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_input"))?,
            stop,
        ),
        _ => anyhow::bail!("capture.invalid_operation"),
    }
}
fn check(stop: &AtomicBool) -> Result<()> {
    ensure!(!stop.load(Ordering::Acquire), "capture.cancelled");
    ensure!(privacy::allowed(), "capture.privacy");
    Ok(())
}
fn targets() -> Result<Value> {
    let mut monitors = Vec::new();
    for monitor in Monitor::enumerate()? {
        use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        ensure!(
            unsafe { GetMonitorInfoW(monitor.as_raw_hmonitor(), &mut info) } != 0,
            "capture.monitor_unavailable"
        );
        monitors.push(json!({"index":monitor.index()?,"name":monitor.name()?,"width":monitor.width()?,"height":monitor.height()?,"left":info.rcMonitor.left,"top":info.rcMonitor.top}));
    }
    let windows = Window::enumerate()?.into_iter().filter_map(|window| {
        Some(json!({"hwnd":window.as_raw_hwnd() as usize as u64,"pid":window.process_id().ok()?,"title":window.title().ok()?,"width":window.width().ok()?,"height":window.height().ok()?}))
    }).collect::<Vec<_>>();
    Ok(
        json!({"monitors":monitors,"windows":windows,"ocr_available":windows::Media::Ocr::OcrEngine::TryCreateFromUserProfileLanguages().is_ok()}),
    )
}
struct CaptureTarget(Target);
impl TryFrom<CaptureTarget> for GraphicsCaptureItemType {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(target: CaptureTarget) -> std::result::Result<Self, Self::Error> {
        match target.0 {
            Target::Monitor { index } => Ok(Monitor::from_index(index)?.try_into()?),
            Target::Window { hwnd, pid } => {
                let window = Window::from_raw_hwnd(usize::try_from(hwnd)? as *mut std::ffi::c_void);
                if window.process_id()? != pid {
                    return Err("capture.window_changed".into());
                }
                Ok(window.try_into()?)
            }
        }
    }
}
fn item(request: &Request) -> Result<(CaptureTarget, u32, u32)> {
    match request
        .target
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("capture.invalid_target"))?
    {
        Target::Monitor { index } => {
            let monitor = Monitor::from_index(*index)?;
            let (width, height) = (monitor.width()?, monitor.height()?);
            capture::validate_size(width, height)?;
            Ok((
                CaptureTarget(Target::Monitor { index: *index }),
                width,
                height,
            ))
        }
        Target::Window { hwnd, pid } => {
            let window = Window::from_raw_hwnd(usize::try_from(*hwnd)? as *mut std::ffi::c_void);
            ensure!(window.is_valid(), "capture.window_unavailable");
            ensure!(window.process_id()? == *pid, "capture.window_changed");
            let (width, height) = (
                u32::try_from(window.width()?)?,
                u32::try_from(window.height()?)?,
            );
            capture::validate_size(width, height)?;
            Ok((
                CaptureTarget(Target::Window {
                    hwnd: *hwnd,
                    pid: *pid,
                }),
                width,
                height,
            ))
        }
    }
}
struct Flags {
    request: Request,
    image: Arc<Mutex<Option<RgbaImage>>>,
    stop: Arc<AtomicBool>,
    width: u32,
    height: u32,
    audio: Option<Arc<audio::Audio>>,
}
struct Capture {
    flags: Flags,
    encoder: Option<VideoEncoder>,
    gif: Option<gif::Encoder<fs::File>>,
    last: Option<RgbaImage>,
    last_at: Duration,
    start: Instant,
    last_frame: Instant,
    frames: u32,
}
impl GraphicsCaptureApiHandler for Capture {
    type Flags = Flags;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn new(context: Context<Flags>) -> std::result::Result<Self, Self::Error> {
        let flags = context.flags;
        Ok(Self {
            flags,
            encoder: None,
            gif: None,
            last: None,
            last_at: Duration::ZERO,
            start: Instant::now(),
            last_frame: Instant::now() - Duration::from_secs(1),
            frames: 0,
        })
    }
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if self.flags.stop.load(Ordering::Acquire) {
            control.stop();
            return Ok(());
        }
        capture::validate_size(frame.width(), frame.height())?;
        let capture_image = self.flags.request.operation != "record";
        if !capture_image && self.last_frame.elapsed() < Duration::from_millis(67) {
            return Ok(());
        }
        self.last_frame = Instant::now();
        let buffer = if let Some(crop) = &self.flags.request.crop {
            crop.validate(frame.width(), frame.height())?;
            frame.buffer_crop(crop.x, crop.y, crop.x + crop.width, crop.y + crop.height)?
        } else {
            frame.buffer()?
        };
        let (width, height) = (buffer.width(), buffer.height());
        let mut packed = Vec::new();
        let raw = buffer.as_nopadding_buffer(&mut packed).to_vec();
        let image: RgbaImage =
            ImageBuffer::from_raw(width, height, raw).ok_or("capture.invalid_pixels")?;
        if capture_image {
            *self.flags.image.lock().unwrap_or_else(|e| e.into_inner()) = Some(image);
            control.stop();
            return Ok(());
        }
        if self.frames == 0 {
            self.initialize_record(width, height)?;
        }
        if let Some(previous) = &self.last
            && previous.dimensions() != image.dimensions()
        {
            return Err("capture.resized".into());
        }
        if self.gif.is_some() {
            self.flush_gif(self.start.elapsed())?;
        } else if let Some(encoder) = &mut self.encoder {
            let pixels = video_pixels(&image, self.flags.width, self.flags.height);
            encoder.send_frame_buffer(&pixels, (self.start.elapsed().as_micros() * 10) as i64)?;
        }
        self.last = Some(image);
        self.last_at = self.start.elapsed();
        self.frames += 1;
        if self.frames >= 1800
            || self.start.elapsed() >= Duration::from_secs(u64::from(self.flags.request.seconds))
        {
            control.stop();
        }
        Ok(())
    }
}
impl Capture {
    fn initialize_record(&mut self, width: u32, height: u32) -> Result<()> {
        self.flags.width = width + if self.flags.request.gif { 0 } else { width % 2 };
        self.flags.height = height
            + if self.flags.request.gif {
                0
            } else {
                height % 2
            };
        capture::validate_size(self.flags.width, self.flags.height)?;

        if self.flags.request.operation == "record" {
            let path = self
                .flags
                .request
                .output
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_output"))?;
            if self.flags.request.gif {
                let mut output = gif::Encoder::new(
                    fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(path)?,
                    self.flags.width as u16,
                    self.flags.height as u16,
                    &[],
                )?;
                output.set_repeat(gif::Repeat::Infinite)?;
                self.gif = Some(output);
            } else {
                self.encoder = Some(VideoEncoder::new(
                    VideoSettingsBuilder::new(self.flags.width, self.flags.height)
                        .frame_rate(15)
                        .bitrate(8_000_000),
                    if let Some(audio) = &self.flags.audio {
                        AudioSettingsBuilder::default()
                            .sample_rate(audio.sample_rate)
                            .channel_count(audio.channels)
                            .bit_per_sample(16)
                    } else {
                        AudioSettingsBuilder::default().disabled(true)
                    },
                    ContainerSettingsBuilder::default(),
                    path,
                )?);
            }
        }

        self.start = Instant::now();
        if let Some(audio) = &self.flags.audio {
            audio.begin();
        }
        Ok(())
    }

    fn flush_gif(&mut self, until: Duration) -> Result<()> {
        if let (Some(encoder), Some(image)) = (&mut self.gif, &self.last) {
            let mut pixels = image.as_raw().clone();
            let mut frame = gif::Frame::from_rgba_speed(
                image.width() as u16,
                image.height() as u16,
                &mut pixels,
                10,
            );
            frame.delay =
                (until.saturating_sub(self.last_at).as_millis() / 10).clamp(1, 12000) as u16;
            encoder.write_frame(&frame)?;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        ensure!(self.frames > 0, "capture.no_frames");
        self.flush_gif(self.start.elapsed())?;
        if let Some(encoder) = self.gif.take() {
            encoder.into_inner()?.sync_all()?;
        }
        if let Some(mut encoder) = self.encoder.take() {
            if let Some(audio) = &self.flags.audio {
                audio.stop();
                audio.drain(&mut encoder)?;
            }
            if let Some(last) = &self.last {
                encoder.send_frame_buffer(
                    &video_pixels(last, self.flags.width, self.flags.height),
                    (self.start.elapsed().as_micros() * 10) as i64,
                )?;
            }
            encoder.finish()?;
        }
        Ok(())
    }
    fn drain_audio(&mut self) -> Result<()> {
        if let (Some(audio), Some(encoder)) = (&self.flags.audio, &mut self.encoder) {
            audio.drain(encoder)?;
        }
        Ok(())
    }
}
fn video_pixels(image: &RgbaImage, width: u32, height: u32) -> Vec<u8> {
    // Raw MF video input is BGRA, bottom-up; pad odd sizes for H.264 rather than crop pixels.
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for (x, y, pixel) in image.enumerate_pixels() {
        let position = ((height - y - 1) * width + x) as usize * 4;
        pixels[position..position + 4].copy_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    pixels
}
fn settings(
    request: &Request,
    flags: Flags,
    item: CaptureTarget,
) -> Settings<Flags, CaptureTarget> {
    Settings::new(
        item,
        if request.cursor {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        },
        DrawBorderSettings::WithBorder,
        SecondaryWindowSettings::Include,
        MinimumUpdateIntervalSettings::Custom(Duration::from_millis(67)),
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        flags,
    )
}
fn screenshot(request: &Request, stop: &Arc<AtomicBool>) -> Result<RgbaImage> {
    check(stop)?;
    let (item, width, height) = item(request)?;
    let image = Arc::new(Mutex::new(None));
    let flags = Flags {
        request: request.clone(),
        image: image.clone(),
        stop: stop.clone(),
        width,
        height,
        audio: None,
    };
    let attempt = (|| -> Result<()> {
        let control = Capture::start_free_threaded(settings(request, flags, item))
            .map_err(|_| anyhow::anyhow!("capture.wgc_failed"))?;
        let start = Instant::now();
        while !control.is_finished() && start.elapsed() < Duration::from_secs(8) {
            if check(stop).is_err() {
                let _ = control.stop();
                anyhow::bail!("capture.privacy");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        control
            .stop()
            .map_err(|_| anyhow::anyhow!("capture.wgc_failed"))?;
        Ok(())
    })();
    check(stop)?;
    if attempt.is_ok()
        && let Some(image) = image.lock().unwrap_or_else(|e| e.into_inner()).take()
    {
        return Ok(image);
    }
    // A window fallback must never silently capture its occluding applications instead.
    let Target::Monitor { index } = request
        .target
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("capture.invalid_target"))?
    else {
        anyhow::bail!("capture.wgc_failed")
    };
    let mut duplication = windows_capture::dxgi_duplication_api::DxgiDuplicationApi::new(
        Monitor::from_index(*index)?,
    )?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        check(stop)?;
        if let Ok(mut frame) = duplication.acquire_next_frame(100) {
            let buffer = if let Some(crop) = &request.crop {
                crop.validate(frame.width(), frame.height())?;
                frame.buffer_crop(crop.x, crop.y, crop.x + crop.width, crop.y + crop.height)?
            } else {
                frame.buffer()?
            };
            let (width, height) = (buffer.width(), buffer.height());
            let mut packed = Vec::new();
            let mut pixels = buffer.as_nopadding_buffer(&mut packed).to_vec();
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            return ImageBuffer::from_raw(width, height, pixels)
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_pixels"));
        }
    }
    anyhow::bail!("capture.no_frames")
}
fn record(request: &Request, stop: &Arc<AtomicBool>) -> Result<Value> {
    check(stop)?;
    let (item, mut width, mut height) = item(request)?;
    if let Some(crop) = &request.crop {
        crop.validate(width, height)?;
        width = crop.width;
        height = crop.height;
    }
    if !request.gif {
        width += width % 2;
        height += height % 2;
    }
    capture::validate_size(width, height)?;
    ensure!(!request.gif || !request.audio, "capture.audio_unavailable");
    let audio = if request.audio {
        Some(audio::Audio::new()?)
    } else {
        None
    };
    let flags = Flags {
        request: request.clone(),
        image: Arc::new(Mutex::new(None)),
        stop: stop.clone(),
        width,
        height,
        audio,
    };
    let control = Capture::start_free_threaded(settings(request, flags, item))
        .map_err(|_| anyhow::anyhow!("capture.record_failed"))?;
    let callback = control.callback();
    let start = Instant::now();
    let mut cancelled = false;
    while !control.is_finished() {
        {
            let mut handler = callback.lock();
            handler.drain_audio()?;
            if handler.frames > 0
                && handler.start.elapsed() >= Duration::from_secs(request.seconds as u64)
            {
                break;
            }
            ensure!(
                handler.frames > 0 || start.elapsed() < Duration::from_secs(8),
                "capture.no_frames"
            );
        }
        if check(stop).is_err() {
            cancelled = true;
            break;
        }
        if request
            .output
            .as_ref()
            .and_then(|p| fs::metadata(p).ok())
            .is_some_and(|m| m.len() > MAX_ASSET_BYTES)
        {
            cancelled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Stopping on explicit user request keeps the completed portion; privacy/sleep never does.
    let user_stop = stop.load(Ordering::Acquire);
    control
        .stop()
        .map_err(|_| anyhow::anyhow!("capture.record_failed"))?;
    ensure!(
        privacy::allowed() && (!cancelled || user_stop),
        "capture.privacy"
    );
    let (duration, width, height) = {
        let mut handler = callback.lock();
        let duration = handler.start.elapsed().as_millis().min(120000) as u64;
        handler.finish()?;
        (duration, handler.flags.width, handler.flags.height)
    };
    Ok(json!({"width":width,"height":height,"duration_ms":duration}))
}
fn open_image(path: &str) -> Result<image::DynamicImage> {
    ensure!(
        fs::metadata(path)?.len() <= MAX_ASSET_BYTES,
        "capture.file_limit"
    );
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    capture::validate_size(width, height)?;
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}
fn qr(path: &str, stop: &AtomicBool) -> Result<Value> {
    check(stop)?;
    let image = open_image(path)?.to_luma8();
    let results = decode_codes(&image)?;
    check(stop)?;
    Ok(json!({"codes":results}))
}
fn decode_codes(image: &image::GrayImage) -> Result<Vec<String>> {
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
        image.width() as usize,
        image.height() as usize,
        |x, y| image.get_pixel(x as u32, y as u32)[0],
    );
    let mut results = Vec::new();
    for grid in prepared.detect_grids().into_iter().take(32) {
        if let Ok((_, text)) = grid.decode() {
            ensure!(text.len() <= 32768, "capture.text_limit");
            results.push(text);
        }
    }
    Ok(results)
}
fn ocr(path: &str, stop: &AtomicBool) -> Result<Value> {
    use windows::{
        Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
    };
    check(stop)?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| anyhow::anyhow!("capture.ocr_missing"))?;
    let mut image = open_image(path)?;
    let maximum = OcrEngine::MaxImageDimension()?;
    if image.width() > maximum || image.height() > maximum {
        image = image.thumbnail(maximum, maximum);
    }
    // Decode a bounded PNG in memory so OCR never follows an arbitrary external file reference.
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    let stream = windows::Storage::Streams::InMemoryRandomAccessStream::new()?;
    let writer = windows::Storage::Streams::DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(png.get_ref())?;
    writer.StoreAsync()?.join()?;
    stream.Seek(0)?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let bitmap = SoftwareBitmap::Convert(
        &decoder.GetSoftwareBitmapAsync()?.join()?,
        BitmapPixelFormat::Bgra8,
    )?;
    let text = engine.RecognizeAsync(&bitmap)?.join()?.Text()?.to_string();
    ensure!(text.encode_utf16().count() <= 65536, "capture.text_limit");
    check(stop)?;
    Ok(json!({"text":text}))
}
fn scroll(request: &Request, stop: &Arc<AtomicBool>) -> Result<Value> {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{INPUT, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput},
        WindowsAndMessaging::GetForegroundWindow,
    };
    let Target::Window { hwnd, pid } = request
        .target
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("capture.invalid_target"))?
    else {
        anyhow::bail!("capture.invalid_target")
    };
    let target = usize::try_from(*hwnd)? as *mut std::ffi::c_void;
    let window = Window::from_raw_hwnd(target);
    ensure!(
        window.is_valid() && window.process_id()? == *pid,
        "capture.window_changed"
    );
    ensure!(
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(target) } != 0,
        "capture.scroll_failed"
    );
    ensure!(
        unsafe { GetForegroundWindow() } == target,
        "capture.cancelled"
    );
    ensure!(!target.is_null(), "capture.invalid_target");
    let mut image = screenshot(request, stop)?;
    let mut previous = image.clone();
    for _ in 0..40 {
        check(stop)?;
        ensure!(
            unsafe { GetForegroundWindow() } == target,
            "capture.cancelled"
        );
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (-480_i32) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        ensure!(
            unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) } == 1,
            "capture.scroll_failed"
        );
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(50));
            check(stop)?;
        }
        let next = screenshot(request, stop)?;
        ensure!(
            next.dimensions() == previous.dimensions(),
            "capture.cancelled"
        );
        let overlap = capture::vertical_overlap(
            previous.as_raw(),
            next.as_raw(),
            next.width(),
            next.height(),
        )?
        .ok_or_else(|| anyhow::anyhow!("capture.no_overlap"))?;
        if overlap == next.height() {
            break;
        }
        let height = image
            .height()
            .checked_add(next.height() - overlap)
            .ok_or_else(|| anyhow::anyhow!("capture.pixel_limit"))?;
        capture::validate_size(image.width(), height)?;
        let mut merged = RgbaImage::new(image.width(), height);
        image::imageops::replace(&mut merged, &image, 0, 0);
        let tail =
            image::imageops::crop_imm(&next, 0, overlap, next.width(), next.height() - overlap)
                .to_image();
        image::imageops::replace(&mut merged, &tail, 0, image.height() as i64);
        image = merged;
        previous = next;
    }
    check(stop)?;
    image.save_with_format(
        Path::new(
            request
                .output
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("capture.invalid_output"))?,
        ),
        image::ImageFormat::Png,
    )?;
    Ok(json!({"width":image.width(),"height":image.height(),"duration_ms":0}))
}
