//! No normal test reads the desktop, microphone, output device or clipboard.
use super::*;

#[test]
fn synthetic_h264_encoding_never_reads_a_screen_or_audio_device() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            unsafe {
                windows::Win32::System::WinRT::RoInitialize(
                    windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                )?;
            }
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("synthetic.mp4");
            fs::File::create_new(&path)?;
            let mut encoder = VideoEncoder::new(
                VideoSettingsBuilder::new(320, 240)
                    .frame_rate(15)
                    .bitrate(1_000_000),
                AudioSettingsBuilder::default().disabled(true),
                ContainerSettingsBuilder::default(),
                &path,
            )?;
            let frame = RgbaImage::from_pixel(320, 240, image::Rgba([240, 40, 70, 255]));
            let bytes = video_pixels(&frame, 320, 240);
            encoder.send_frame_buffer(&bytes, 0)?;
            encoder.send_frame_buffer(&bytes, 10_000_000)?;
            encoder.finish()?;
            let video = fs::read(&path)?;
            ensure!(
                video.len() > 1000 && &video[4..8] == b"ftyp",
                "synthetic video missing"
            );
            Ok(())
        })()
        .map_err(|error| format!("{error:?}"));
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(Duration::from_secs(25))
        .expect("synthetic encoder timed out")
        .unwrap();
}

#[test]
fn synthetic_pixels_roundtrip_png_qr_video_layout_and_gif_timing() {
    let dir = tempfile::tempdir().unwrap();
    let qr =
        qrcodegen::QrCode::encode_text("Moodflow synthetic QR 123", qrcodegen::QrCodeEcc::Medium)
            .unwrap();
    let side = ((qr.size() + 8) * 8) as u32;
    let pixels = image::GrayImage::from_fn(side, side, |x, y| {
        image::Luma([if qr.get_module(x as i32 / 8 - 4, y as i32 / 8 - 4) {
            0
        } else {
            255
        }])
    });
    let path = dir.path().join("fixture.png");
    pixels.save(&path).unwrap();
    let decoded = open_image(path.to_str().unwrap()).unwrap().to_luma8();
    assert_eq!(
        decode_codes(&decoded).unwrap(),
        ["Moodflow synthetic QR 123"]
    );
    fs::write(dir.path().join("broken.png"), b"not an image").unwrap();
    assert!(open_image(dir.path().join("broken.png").to_str().unwrap()).is_err());

    let image = RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8, y as u8, 99, 255]));
    let video = video_pixels(&image, 4, 2);
    assert_eq!(&video[..4], &[99, 1, 0, 255]);
    assert_eq!(&video[16..20], &[99, 0, 0, 255]);
    assert_eq!(&video[12..16], &[0, 0, 0, 0]);

    let output = dir.path().join("fixture.gif");
    fs::File::create_new(&output).unwrap();
    let request: Request = serde_json::from_value(
        json!({"operation":"record", "gif":true, "seconds":2, "output":output}),
    )
    .unwrap();
    let mut handler = Capture {
        flags: Flags {
            request,
            image: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            width: 3,
            height: 2,
            audio: None,
        },
        encoder: None,
        gif: None,
        last: None,
        last_at: Duration::ZERO,
        start: Instant::now(),
        last_frame: Instant::now(),
        frames: 0,
    };
    handler.initialize_record(3, 2).unwrap();
    handler.last = Some(image);
    handler.flush_gif(Duration::from_millis(750)).unwrap();
    handler.last_at = Duration::from_millis(750);
    handler.start = Instant::now() - Duration::from_secs(1);
    handler.frames = 1;
    handler.finish().unwrap();
    let mut decoder = gif::DecodeOptions::new()
        .read_info(fs::File::open(output).unwrap())
        .unwrap();
    assert_eq!(decoder.width(), 3);
    assert_eq!(decoder.read_next_frame().unwrap().unwrap().delay, 75);
    assert!(decoder.read_next_frame().unwrap().unwrap().delay >= 25);
    assert!(decoder.read_next_frame().unwrap().is_none());
}

#[test]
#[ignore = "creates/captures only a synthetic window on an isolated Windows CI desktop"]
fn isolated_window_capture_ocr_and_h264() {
    // Check before opening any window or requesting capture. This must never record a user's desktop.
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    use windows_sys::Win32::{Foundation::HWND, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        unsafe {
            match message {
                WM_TIMER => {
                    InvalidateRect(window, std::ptr::null(), 0);
                    0
                }
                WM_PAINT => {
                    let mut paint: PAINTSTRUCT = std::mem::zeroed();
                    let dc = BeginPaint(window, &mut paint);
                    let mut bounds = std::mem::zeroed();
                    GetClientRect(window, &mut bounds);
                    FillRect(dc, &bounds, GetStockObject(WHITE_BRUSH) as HBRUSH);
                    let font = CreateFontW(
                        48,
                        0,
                        0,
                        0,
                        700,
                        0,
                        0,
                        0,
                        1,
                        0,
                        0,
                        0,
                        0,
                        windows::core::w!("Segoe UI").as_ptr(),
                    );
                    let old = SelectObject(dc, font);
                    let text = "MOODFLOW 12345".encode_utf16().collect::<Vec<_>>();
                    TextOutW(dc, 30, 55, text.as_ptr(), text.len() as i32);
                    SelectObject(dc, old);
                    DeleteObject(font);
                    static FRAME: std::sync::atomic::AtomicU32 =
                        std::sync::atomic::AtomicU32::new(0);
                    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
                    let rectangle = windows_sys::Win32::Foundation::RECT {
                        left: 600,
                        top: 200,
                        right: 630,
                        bottom: 230,
                    };
                    let brush = CreateSolidBrush(if frame.is_multiple_of(2) {
                        0x00AA5500
                    } else {
                        0x000055AA
                    });
                    FillRect(dc, &rectangle, brush);
                    DeleteObject(brush);
                    EndPaint(window, &paint);
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let ui = std::thread::spawn(move || unsafe {
        let class = windows::core::w!("MoodflowCaptureSyntheticTest");
        let spec = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        assert_ne!(RegisterClassW(&spec), 0);
        let handle = CreateWindowExW(
            0,
            class.as_ptr(),
            windows::core::w!("Moodflow synthetic capture").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            100,
            100,
            640,
            240,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        assert!(!handle.is_null());
        UpdateWindow(handle);
        SetTimer(handle, 1, 100, None);
        sender.send(handle as usize).unwrap();
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnregisterClassW(class.as_ptr(), std::ptr::null_mut());
    });
    let handle = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    struct Close(usize);
    impl Drop for Close {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.0 as HWND, WM_CLOSE, 0, 0);
            }
        }
    }
    let close = Close(handle);
    unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
        .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.png");
    let request: Request = serde_json::from_value(json!({"operation":"screenshot", "target":{"kind":"window", "hwnd":handle as u64,"pid":std::process::id()}, "output":path})).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let image = screenshot(&request, &stop).unwrap();
    assert_eq!(image.dimensions(), (640, 240));
    image.save(&path).unwrap();
    let text = ocr(path.to_str().unwrap(), &stop).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_uppercase();
    assert!(
        text.contains("MOODFLOW") && text.contains("12345"),
        "synthetic text was not recognized"
    );
    let video_path = dir.path().join("synthetic.mp4");
    fs::File::create_new(&video_path).unwrap();
    let recording: Request = serde_json::from_value(
        json!({"operation":"record", "target":request.target, "seconds":1,"output":video_path}),
    )
    .unwrap();
    let result = record(&recording, &stop).unwrap();
    assert!(result["duration_ms"].as_u64().unwrap() >= 900);
    let video = fs::read(video_path).unwrap();
    assert!(video.len() > 1000 && &video[4..8] == b"ftyp");
    assert!(video.windows(4).any(|v| v == b"moov") && video.windows(4).any(|v| v == b"mdat"));
    drop(close);
    ui.join().unwrap();
}
