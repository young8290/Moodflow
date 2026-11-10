//! Explicit system-output loopback only; never opens a microphone.
use anyhow::{Result, ensure};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};
use windows::Win32::{
    Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
        WAVEFORMATEXTENSIBLE, eConsole, eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    },
};
pub struct Audio {
    pub sample_rate: u32,
    pub channels: u32,
    receiver: Mutex<Receiver<Vec<u8>>>,
    begin: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl Audio {
    pub fn new() -> Result<Arc<Self>> {
        let begin = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(64);
        let (ready, info) = mpsc::sync_channel(1);
        let (thread_begin, thread_stop, thread_failed) =
            (begin.clone(), stop.clone(), failed.clone());
        let thread = std::thread::spawn(move || {
            if pump(sender, ready, &thread_begin, &thread_stop).is_err() {
                thread_failed.store(true, Ordering::Release);
            }
        });
        let (sample_rate, channels) = match info.recv_timeout(Duration::from_secs(5)) {
            Ok(format) => format,
            Err(_) => {
                stop.store(true, Ordering::Release);
                anyhow::bail!("capture.audio_unavailable");
            }
        };
        Ok(Arc::new(Self {
            sample_rate,
            channels,
            receiver: Mutex::new(receiver),
            begin,
            stop,
            failed,
            thread: Mutex::new(Some(thread)),
        }))
    }
    pub fn begin(&self) {
        self.begin.store(true, Ordering::Release);
    }
    pub fn drain(&self, encoder: &mut windows_capture::encoder::VideoEncoder) -> Result<()> {
        ensure!(
            !self.failed.load(Ordering::Acquire),
            "capture.audio_unavailable"
        );
        for buffer in self
            .receiver
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_iter()
        {
            encoder.send_audio_buffer(&buffer, 0)?;
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
    }
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Mix(*mut WAVEFORMATEX);
impl Drop for Mix {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.cast()));
        }
    }
}
fn pump(
    sender: SyncSender<Vec<u8>>,
    ready: SyncSender<(u32, u32)>,
    begin: &AtomicBool,
    stop: &AtomicBool,
) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let _com = Com;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole)? };
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let mix = Mix(unsafe { client.GetMixFormat()? });
    ensure!(!mix.0.is_null(), "capture.audio_unavailable");
    let format = unsafe { *mix.0 };
    let (channels, sample_rate, bits) = (
        format.nChannels as usize,
        format.nSamplesPerSec,
        format.wBitsPerSample,
    );
    let mut kind = format.wFormatTag as u32;
    if kind == 65534 {
        ensure!(format.cbSize >= 22, "capture.audio_unavailable");
        kind = unsafe { (*mix.0.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat.data1 };
    }
    ensure!(
        (1..=8).contains(&channels)
            && (8000..=192000).contains(&sample_rate)
            && ((kind == 3 && bits == 32) || (kind == 1 && [16, 32].contains(&bits)))
            && format.nBlockAlign as usize == channels * bits as usize / 8,
        "capture.audio_unavailable"
    );
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            1_000_000,
            0,
            mix.0,
            None,
        )?;
    }
    drop(mix);
    let capture: IAudioCaptureClient = unsafe { client.GetService()? };
    ready
        .send((sample_rate, if channels == 1 { 1 } else { 2 }))
        .map_err(|_| anyhow::anyhow!("capture.cancelled"))?;
    while !begin.load(Ordering::Acquire) {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    unsafe {
        client.Start()?;
    }
    let result = (|| -> Result<()> {
        while !stop.load(Ordering::Acquire) {
            while unsafe { capture.GetNextPacketSize()? } > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                unsafe {
                    capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                }
                let result = (|| -> Result<Vec<u8>> {
                    ensure!(frames <= 65536, "capture.audio_unavailable");
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        return Ok(vec![0; frames as usize * channels.min(2) * 2]);
                    }
                    ensure!(!data.is_null(), "capture.audio_unavailable");
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            data,
                            frames as usize * channels * bits as usize / 8,
                        )
                    };
                    pcm16(bytes, channels, bits, kind == 3)
                })();
                unsafe {
                    capture.ReleaseBuffer(frames)?;
                }
                sender
                    .try_send(result?)
                    .map_err(|_| anyhow::anyhow!("capture.audio_unavailable"))?;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    let _ = unsafe { client.Stop() };
    result
}
fn pcm16(input: &[u8], channels: usize, bits: u16, float: bool) -> Result<Vec<u8>> {
    let sample_bytes = bits as usize / 8;
    ensure!(
        (1..=8).contains(&channels)
            && [16, 32].contains(&bits)
            && (!float || bits == 32)
            && input.len().is_multiple_of(channels * sample_bytes),
        "capture.audio_unavailable"
    );
    let mut output = Vec::with_capacity(input.len());
    for frame in input.chunks_exact(channels * sample_bytes) {
        let read = |channel: usize| -> f32 {
            let at = channel * sample_bytes;
            let sample = if float {
                f32::from_le_bytes(frame[at..at + 4].try_into().unwrap())
            } else if bits == 16 {
                i16::from_le_bytes(frame[at..at + 2].try_into().unwrap()) as f32 / 32768.0
            } else {
                i32::from_le_bytes(frame[at..at + 4].try_into().unwrap()) as f32 / 2147483648.0
            };
            if sample.is_finite() { sample } else { 0.0 }
        };
        for side in 0..channels.min(2) {
            let mut sample = read(side);
            if channels > 2 {
                let mut weight = 1.0;
                for index in 2..channels {
                    let contribution = match index {
                        2 => 0.707,
                        3 => 0.5,
                        n if n % 2 == side => 0.707,
                        _ => 0.0,
                    };
                    sample += read(index) * contribution;
                    weight += contribution;
                }
                sample /= weight;
            }
            output.extend_from_slice(
                &((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes(),
            );
        }
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_conversion_is_bounded_without_device_access() {
        let input = [1_f32, -1.0, f32::NAN, f32::INFINITY]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let output = pcm16(&input, 2, 32, true).unwrap();
        assert_eq!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| i16::from_le_bytes(*v))
                .collect::<Vec<_>>(),
            [32767, -32767, 0, 0]
        );
        assert_eq!(pcm16(&[0; 6 * 4], 6, 32, true).unwrap().len(), 4);
        assert!(pcm16(&[0; 3], 2, 32, true).is_err());
    }
}
