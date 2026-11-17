//! PC sound capture that ignores the speaker volume.
//!
//! Classic WASAPI loopback taps the speaker output. On many PCs (e.g. Intel
//! Smart Sound laptops) that tap sits after the volume slider, so a quiet
//! speaker means a quiet recording. "Process loopback" (Windows 10 build
//! 20348+ / Windows 11) instead captures every app's sound before the
//! speaker volume is applied, so the recording keeps its real
//! level whatever the speaker is set to. SRec's own process tree is
//! excluded (it plays nothing anyway).
//!
//! On older Windows builds `start` returns an error and the caller falls
//! back to classic loopback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{implement, IUnknown, Interface, HRESULT, PROPVARIANT};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const RATE: u32 = 48_000;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const VT_BLOB: u16 = 65;
const BUFFERFLAGS_SILENT: u32 = 2;

/// PROPVARIANT holding a BLOB (x64 layout), as ActivateAudioInterfaceAsync wants.
#[repr(C)]
struct BlobVariant {
    vt: u16,
    reserved: [u16; 3],
    size: u32,
    _pad: u32,
    data: *const u8,
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completed(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completed_Impl {
    fn ActivateCompleted(&self, _op: Option<&IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let (done, cv) = &*self.0;
        if let Ok(mut d) = done.lock() {
            *d = true;
        }
        cv.notify_all();
        Ok(())
    }
}

/// Starts capturing all PC sound at 48 kHz stereo f32. `push` gets each
/// packet; the thread ends when `stop` is set.
pub fn start<F>(mut push: F, stop: Arc<AtomicBool>) -> Result<JoinHandle<()>, String>
where
    F: FnMut(&[f32]) + Send + 'static,
{
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let handle = std::thread::Builder::new()
        .name("pc-sound".into())
        .spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let (client, capture, event) = match open() {
                Ok(x) => {
                    let _ = ready_tx.send(Ok(()));
                    x
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let mut silence: Vec<f32> = Vec::new();
            'outer: while !stop.load(Ordering::Relaxed) {
                WaitForSingleObject(event, 100);
                loop {
                    let n = match capture.GetNextPacketSize() {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break 'outer,
                    };
                    let _ = n;
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    if capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                        .is_err()
                    {
                        break 'outer;
                    }
                    let len = frames as usize * 2;
                    if flags & BUFFERFLAGS_SILENT != 0 || data.is_null() {
                        silence.resize(len, 0.0);
                        push(&silence[..len]);
                    } else {
                        push(std::slice::from_raw_parts(data as *const f32, len));
                    }
                    let _ = capture.ReleaseBuffer(frames);
                }
            }
            let _ = client.Stop();
            let _ = CloseHandle(event);
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => Err("process loopback did not start".into()),
    }
}

unsafe fn open() -> Result<(IAudioClient, IAudioCaptureClient, windows::Win32::Foundation::HANDLE), String> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let pv = BlobVariant {
        vt: VT_BLOB,
        reserved: [0; 3],
        size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        _pad: 0,
        data: &params as *const _ as *const u8,
    };
    let signal = Arc::new((Mutex::new(false), Condvar::new()));
    let handler: IActivateAudioInterfaceCompletionHandler = Completed(signal.clone()).into();
    let op = ActivateAudioInterfaceAsync(
        VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        &IAudioClient::IID,
        Some(&pv as *const BlobVariant as *const PROPVARIANT),
        &handler,
    )
    .map_err(|e| format!("process loopback unavailable: {e}"))?;

    // Wait for the activation callback (normally a few milliseconds).
    {
        let (done, cv) = &*signal;
        let g = done.lock().map_err(|_| "lock")?;
        let (g, _) = cv
            .wait_timeout_while(g, Duration::from_secs(2), |d| !*d)
            .map_err(|_| "lock")?;
        if !*g {
            return Err("process loopback activation timed out".into());
        }
    }
    let mut hr = HRESULT(0);
    let mut unk: Option<IUnknown> = None;
    op.GetActivateResult(&mut hr, &mut unk).map_err(|e| e.to_string())?;
    hr.ok().map_err(|e| format!("process loopback refused: {e}"))?;
    let client: IAudioClient = unk.ok_or("no audio client")?.cast().map_err(|e| e.to_string())?;

    let fmt = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: 2,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 8,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            2_000_000, // 200 ms buffer
            0,
            &fmt,
            None,
        )
        .map_err(|e| format!("process loopback init: {e}"))?;
    let event = CreateEventW(None, false, false, None).map_err(|e| e.to_string())?;
    client.SetEventHandle(event).map_err(|e| e.to_string())?;
    let capture: IAudioCaptureClient = client.GetService().map_err(|e| e.to_string())?;
    client.Start().map_err(|e| e.to_string())?;
    Ok((client, capture, event))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual check: `SREC_LOOPBACK_SECS=4 cargo test loopback -- --nocapture`
    /// while something plays. Prints the captured peak level.
    #[test]
    fn loopback_captures() {
        let secs: u64 = std::env::var("SREC_LOOPBACK_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if secs == 0 {
            return;
        }
        let peak = Arc::new(Mutex::new((0f32, 0usize)));
        let p2 = peak.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let h = start(
            move |d| {
                let mut g = p2.lock().unwrap();
                g.1 += d.len();
                for x in d {
                    g.0 = g.0.max(x.abs());
                }
            },
            stop.clone(),
        )
        .expect("process loopback should start on this PC");
        std::thread::sleep(Duration::from_secs(secs));
        stop.store(true, Ordering::SeqCst);
        h.join().unwrap();
        let g = peak.lock().unwrap();
        println!("LOOPBACK samples={} peak={:.4}", g.1, g.0);
    }
}
