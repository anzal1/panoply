//! Speech synthesis through `AVSpeechSynthesizer`, behind a safe API.
//!
//! AVFAudio is opened with `dlopen` at the first call and its classes are found by name at run
//! time, like Apple Vision in [`crate::ocr`]. Nothing here links the framework, and nothing is
//! resident until a job asks for a voice list or a render.
//!
//! Rendering uses `write:toBufferCallback:`, which hands over the synthesized audio as PCM
//! buffers instead of playing it. The buffers are moved to the caller's thread through a
//! channel, so the caller sees plain 16 bit mono or stereo samples and can stop between
//! buffers. This module never plays anything.
//!
//! **The main run loop.** AVSpeechSynthesizer delivers the buffer callback on the main queue
//! (measured on macOS 26.6: a worker thread that waits with the main thread idle gets nothing).
//! In the desktop app the main thread runs the application loop, so a job on a worker thread
//! just waits for the buffers. A command line process that calls this on its main thread has
//! nobody else to drain the queue, so on the main thread this pumps the run loop itself while
//! it waits. A process that runs its work on a worker thread while the main thread blocks
//! (`panoply mcp`) must pump the main loop there with [`pump_main_run_loop`]. Called from a
//! worker thread of a process whose main thread is blocked, a render times out after 30
//! seconds with a plain error instead of hanging.

use std::ffi::{c_char, c_int, c_void};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::CFString;

use crate::error::Error;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn pthread_main_np() -> c_int;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: u8) -> i32;
    static kCFRunLoopDefaultMode: *const c_void;
}

/// `RTLD_LAZY` on macOS.
const RTLD_LAZY: c_int = 0x1;

/// `AVSpeechBoundaryImmediate`.
const BOUNDARY_IMMEDIATE: isize = 0;

/// How long to wait for the next buffer before giving up on a render. Apple's synthesizer
/// delivers its first buffer within a second or two even on a cold start, so 30 seconds means
/// nothing is draining the main queue.
const STALL: Duration = Duration::from_secs(30);

/// One system voice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// The voice's identifier, for example `com.apple.voice.compact.en-US.Samantha`.
    pub id: String,
    /// The name shown to people, for example `Samantha`.
    pub name: String,
    /// A BCP 47 language tag, for example `en-US`.
    pub language: String,
    /// 1 default, 2 enhanced, 3 premium.
    pub quality: u8,
}

/// A block of synthesized audio: 16 bit samples, interleaved when there are two channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    /// Samples per second.
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// The samples.
    pub samples: Vec<i16>,
}

/// What to render.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// A voice identifier, or a language tag such as `hi-IN`, or `None` for the system default.
    pub voice: Option<&'a str>,
    /// Apple's rate, 0.0 to 1.0, where 0.5 is the default speed.
    pub rate: f32,
}

fn open_avf() -> bool {
    static OPEN: OnceLock<bool> = OnceLock::new();
    *OPEN.get_or_init(|| {
        // SAFETY: the path is a NUL terminated literal, and loading a system framework has no
        // preconditions.
        let handle = unsafe {
            dlopen(
                c"/System/Library/Frameworks/AVFAudio.framework/AVFAudio".as_ptr(),
                RTLD_LAZY,
            )
        };
        !handle.is_null()
    })
}

/// True when the speech classes can be found.
pub fn available() -> bool {
    open_avf() && AnyClass::get(c"AVSpeechSynthesizer").is_some()
}

fn class(name: &'static std::ffi::CStr) -> Result<&'static AnyClass, Error> {
    if !open_avf() {
        return Err(Error::new("The system speech engine is not available."));
    }
    AnyClass::get(name).ok_or_else(|| Error::new("The system speech engine is missing a part."))
}

/// An NSString as a Rust string.
fn ns_string(s: &AnyObject) -> String {
    // SAFETY: NSString and CFString are toll free bridged, and the caller passes an NSString.
    let cf: &CFString = unsafe { &*(s as *const AnyObject).cast::<CFString>() };
    cf.to_string()
}

fn string_prop(o: &AnyObject, get: objc2::runtime::Sel) -> String {
    // SAFETY: the caller passes the selector of a no argument getter that returns an NSString.
    let s: Option<Retained<AnyObject>> = unsafe { msg_send![o, performSelector: get] };
    s.map(|s| ns_string(&s)).unwrap_or_default()
}

fn voice_of(v: &AnyObject) -> Voice {
    // SAFETY: `quality` takes no arguments and returns an NSInteger.
    let q: isize = unsafe { msg_send![v, quality] };
    Voice {
        id: string_prop(v, objc2::sel!(identifier)),
        name: string_prop(v, objc2::sel!(name)),
        language: string_prop(v, objc2::sel!(language)),
        quality: q.clamp(1, 3) as u8,
    }
}

/// The voices installed on this Mac.
pub fn voices() -> Vec<Voice> {
    let Ok(cls) = class(c"AVSpeechSynthesisVoice") else {
        return Vec::new();
    };
    autoreleasepool(|_| {
        // SAFETY: `speechVoices` is a class method with no arguments returning an NSArray.
        let arr: Option<Retained<AnyObject>> = unsafe { msg_send![cls, speechVoices] };
        let Some(arr) = arr else {
            return Vec::new();
        };
        // SAFETY: `count` takes no arguments and returns an NSUInteger.
        let n: usize = unsafe { msg_send![&*arr, count] };
        (0..n)
            .filter_map(|i| {
                // SAFETY: `objectAtIndex:` with an in range index returns a voice.
                let v: Option<Retained<AnyObject>> = unsafe { msg_send![&*arr, objectAtIndex: i] };
                v.map(|v| voice_of(&v))
            })
            .collect()
    })
}

fn ns(s: &str) -> Retained<AnyObject> {
    let cf = CFString::from_str(s);
    let ptr: *mut AnyObject = (&*cf as *const CFString).cast::<AnyObject>().cast_mut();
    // SAFETY: an NSString and a CFString are the same object under toll free bridging, `ptr`
    // is live (held by `cf`), and `retain` gives us our own +1 reference that outlives `cf`.
    unsafe { Retained::retain(ptr) }.expect("a CFString pointer is never null")
}

/// Runs the calling thread's run loop for up to `seconds`, which delivers whatever waits on the
/// main queue. Call it from the main thread only, in a loop, while another thread works. It
/// returns at once when the loop has nothing to wait for, so it sleeps a few milliseconds then
/// instead of spinning.
pub fn pump_main_run_loop(seconds: f64) {
    // SAFETY: running the current thread's loop in the default mode for a short time has no
    // preconditions. `kCFRunLoopDefaultMode` is a constant CFString the system provides.
    let status = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 0) };
    // kCFRunLoopRunFinished: no sources or timers, so the call did not wait.
    if status == 1 {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Renders `text` and hands every block of audio to `sink`, in order, on the calling thread.
/// `sink` returns `false` to stop (a cancel): the synthesizer is told to stop and this returns
/// `Ok` with what was delivered so far. Returns an error when no audio arrives for 30 seconds.
pub fn synthesize(
    text: &str,
    request: &Request<'_>,
    sink: &mut dyn FnMut(Pcm) -> bool,
) -> Result<(), Error> {
    let utterance_class = class(c"AVSpeechUtterance")?;
    let synth_class = class(c"AVSpeechSynthesizer")?;
    let voice_class = class(c"AVSpeechSynthesisVoice")?;

    enum Msg {
        Audio(Pcm),
        Done,
    }
    let (tx, rx) = mpsc::channel::<Msg>();

    autoreleasepool(|_| {
        let s = ns(text);
        // SAFETY: `speechUtteranceWithString:` takes an NSString and returns an utterance.
        let utt: Retained<AnyObject> =
            unsafe { msg_send![utterance_class, speechUtteranceWithString: &*s] };
        let rate = request.rate.clamp(0.0, 1.0);
        // SAFETY: `setRate:` takes one float.
        unsafe {
            let _: () = msg_send![&*utt, setRate: rate];
        }
        if let Some(v) = request.voice {
            let key = ns(v);
            // SAFETY: both class methods take an NSString and return a nullable voice. An
            // identifier is tried first, then a language tag.
            let voice: Option<Retained<AnyObject>> = unsafe {
                let by_id: Option<Retained<AnyObject>> =
                    msg_send![voice_class, voiceWithIdentifier: &*key];
                by_id.or_else(|| msg_send![voice_class, voiceWithLanguage: &*key])
            };
            let Some(voice) = voice else {
                return Err(Error::new(format!(
                    "There is no voice called \"{v}\" on this Mac. Run the voice list to see the installed ones."
                )));
            };
            // SAFETY: `setVoice:` takes a voice.
            unsafe {
                let _: () = msg_send![&*utt, setVoice: &*voice];
            }
        }
        // SAFETY: `new` takes no arguments and returns a retained synthesizer.
        let synth: Retained<AnyObject> = unsafe { msg_send![synth_class, new] };

        let tx_cb = std::sync::Mutex::new(tx.clone());
        let block = RcBlock::new(move |buffer: *mut AnyObject| {
            let tx = tx_cb.lock().unwrap_or_else(|p| p.into_inner());
            // SAFETY: the synthesizer passes a live AVAudioBuffer for the length of the call.
            let msg = match unsafe { buffer.as_ref() }.and_then(pcm_of) {
                Some(p) if !p.samples.is_empty() => Msg::Audio(p),
                _ => Msg::Done,
            };
            let _ = tx.send(msg);
        });
        // SAFETY: `write:toBufferCallback:` takes an utterance and a block that takes one
        // AVAudioBuffer. The block is copied by the synthesizer, so it outlives this call.
        unsafe {
            let _: () = msg_send![&*synth, writeUtterance: &*utt, toBufferCallback: &*block];
        }
        drop(tx);

        let mut last = Instant::now();
        let mut got_audio = false;
        // SAFETY: `pthread_main_np` takes no arguments and only reads thread state.
        let on_main = unsafe { pthread_main_np() } != 0;
        loop {
            let next = if on_main {
                // Take what has already arrived first: one pump of the loop delivers many
                // buffers at once, and waiting a pump per buffer made a render several times
                // slower than real time.
                match rx.try_recv() {
                    Ok(m) => Ok(m),
                    Err(mpsc::TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
                    Err(mpsc::TryRecvError::Empty) => {
                        pump_main_run_loop(0.02);
                        Err(RecvTimeoutError::Timeout)
                    }
                }
            } else {
                rx.recv_timeout(Duration::from_millis(250))
            };
            match next {
                Ok(Msg::Audio(p)) => {
                    got_audio = true;
                    last = Instant::now();
                    if !sink(p) {
                        // SAFETY: `stopSpeakingAtBoundary:` takes an NSInteger and returns BOOL.
                        unsafe {
                            let _: bool =
                                msg_send![&*synth, stopSpeakingAtBoundary: BOUNDARY_IMMEDIATE];
                        }
                        return Ok(());
                    }
                }
                Ok(Msg::Done) | Err(RecvTimeoutError::Disconnected) => {
                    return if got_audio {
                        Ok(())
                    } else {
                        Err(Error::new(
                            "The voice produced no sound. Pick another voice or check the text.",
                        ))
                    };
                }
                Err(RecvTimeoutError::Timeout) => {
                    if last.elapsed() > STALL {
                        // SAFETY: as above.
                        unsafe {
                            let _: bool =
                                msg_send![&*synth, stopSpeakingAtBoundary: BOUNDARY_IMMEDIATE];
                        }
                        return Err(Error::new(
                            "The system voice did not answer. Try again, or pick another voice.",
                        ));
                    }
                    // A cancel while waiting for a slow first buffer: an empty-handed sink
                    // call is not possible, so cancellation is checked at the next buffer.
                }
            }
        }
    })
}

/// An `AVAudioPCMBuffer` as 16 bit samples. `None` for anything that is not PCM.
fn pcm_of(buf: &AnyObject) -> Option<Pcm> {
    // SAFETY: `frameLength` is an AVAudioPCMBuffer property (UInt32); an AVAudioBuffer of
    // another kind does not respond, which the check below catches first.
    let responds: bool = unsafe { msg_send![buf, respondsToSelector: objc2::sel!(frameLength)] };
    if !responds {
        return None;
    }
    // SAFETY: `frameLength` takes no arguments and returns a UInt32.
    let frames: u32 = unsafe { msg_send![buf, frameLength] };
    if frames == 0 {
        return None;
    }
    // SAFETY: `format` returns the buffer's AVAudioFormat.
    let format: Retained<AnyObject> = unsafe { msg_send![buf, format] };
    // SAFETY: `sampleRate` returns a double, `channelCount` a UInt32.
    let (rate, channels): (f64, u32) = unsafe {
        (
            msg_send![&*format, sampleRate],
            msg_send![&*format, channelCount],
        )
    };
    let channels = channels.clamp(1, 2) as usize;
    let frames = frames as usize;
    // SAFETY: each channel data getter returns a pointer to one array per channel, each
    // `frameLength` samples long, or null when the buffer holds the other sample type.
    let f32_data: *const *const f32 = unsafe { msg_send![buf, floatChannelData] };
    let mut samples = Vec::with_capacity(frames * channels);
    if let Some(chs) = NonNull::new(f32_data as *mut *const f32) {
        for i in 0..frames {
            for c in 0..channels {
                // SAFETY: channel `c` of `channels` has `frames` samples (see above).
                let v = unsafe { *chs.as_ptr().add(c).read().add(i) };
                samples.push((v.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
        }
    } else {
        // SAFETY: as above, for 16 bit data.
        let i16_data: *const *const i16 = unsafe { msg_send![buf, int16ChannelData] };
        let chs = NonNull::new(i16_data as *mut *const i16)?;
        for i in 0..frames {
            for c in 0..channels {
                // SAFETY: channel `c` of `channels` has `frames` samples (see above).
                samples.push(unsafe { *chs.as_ptr().add(c).read().add(i) });
            }
        }
    }
    Some(Pcm {
        sample_rate: rate.round() as u32,
        channels: channels as u16,
        samples,
    })
}
