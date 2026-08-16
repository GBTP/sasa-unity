//! sasa 的 C ABI 封装，供 Unity P/Invoke 调用。
//!
//! 线程约定：所有导出函数只能从 Unity 主线程调用。引擎实例存放在 thread_local 里——
//! cpal 的 `Stream` 是 `!Send`，`AudioManager` 因此无法跨线程移动，这不是可以放宽的
//! 约定。Music/Sfx 句柄内部也是单生产者通道，并发调用会破坏其状态。
//! 音频渲染在 sasa 自己的回调线程上进行，与这里的调用通过通道解耦。

mod error;

use error::{clear_error, fallible, set_error};
use sasa::{AudioClip, AudioManager, Frame, Music, MusicParams, PlaySfxParams, Sfx};
use std::{
    cell::RefCell,
    os::raw::{c_float, c_uchar, c_uint, c_void},
    ptr::null_mut,
    slice,
};

thread_local! {
    static MANAGER: RefCell<Option<AudioManager>> = const { RefCell::new(None) };
}

type Handle = *mut c_void;

/// SAFETY: 调用方必须保证 `handle` 由对应的 `*Create` 函数返回且尚未销毁。
unsafe fn as_ref<'a, T>(handle: Handle) -> Option<&'a mut T> {
    (handle as *mut T).as_mut()
}

fn into_handle<T>(value: T) -> Handle {
    Box::into_raw(Box::new(value)) as Handle
}

/// SAFETY: 同 `as_ref`；调用后 `handle` 失效。
unsafe fn drop_handle<T>(handle: Handle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle as *mut T));
    }
}

/// 借用引擎实例；未初始化时记录错误并返回 `default`。
fn with_manager<T>(default: T, f: impl FnOnce(&mut AudioManager) -> T) -> T {
    MANAGER.with_borrow_mut(|slot| match slot.as_mut() {
        Some(manager) => f(manager),
        None => {
            set_error("audio manager not initialized");
            default
        }
    })
}

// ============================================================
// 引擎
// ============================================================

/// `sample_rate` / `buffer_size` 传 0 表示交给后端自行决定。
#[no_mangle]
pub extern "C" fn SasaUnity_Initialize(sample_rate: c_uint, buffer_size: c_uint) -> c_uchar {
    clear_error();

    MANAGER.with_borrow_mut(|slot| {
        if slot.is_some() {
            return 1;
        }

        let opt = |v: c_uint| if v == 0 { None } else { Some(v) };

        match create_manager(opt(sample_rate), opt(buffer_size)) {
            Ok(manager) => {
                *slot = Some(manager);
                1
            }
            Err(e) => {
                set_error(e);
                0
            }
        }
    })
}

#[cfg(target_os = "android")]
fn create_manager(
    _sample_rate: Option<u32>,
    buffer_size: Option<u32>,
) -> anyhow::Result<AudioManager> {
    use sasa::backend::oboe::{OboeBackend, OboeSettings, PerformanceMode, Usage};
    AudioManager::new(OboeBackend::new(OboeSettings {
        buffer_size,
        performance_mode: PerformanceMode::LowLatency,
        usage: Usage::Game,
    }))
}

#[cfg(not(target_os = "android"))]
fn create_manager(
    sample_rate: Option<u32>,
    buffer_size: Option<u32>,
) -> anyhow::Result<AudioManager> {
    use sasa::backend::cpal::{CpalBackend, CpalSettings};
    AudioManager::new(CpalBackend::new(CpalSettings {
        preferred_sample_rate: sample_rate,
        buffer_size,
    }))
}

#[no_mangle]
pub extern "C" fn SasaUnity_Shutdown() {
    clear_error();
    MANAGER.with_borrow_mut(|slot| *slot = None);
}

#[no_mangle]
pub extern "C" fn SasaUnity_IsValid() -> c_uchar {
    MANAGER.with_borrow(|slot| u8::from(slot.is_some()))
}

/// 每帧调用。设备被抢占（拔耳机、来电）后 sasa 会标记 broken，这里负责重开流。
#[no_mangle]
pub extern "C" fn SasaUnity_RecoverIfNeeded() -> c_uchar {
    clear_error();
    with_manager(0, |manager| {
        fallible(
            || {
                manager.recover_if_needed()?;
                Ok(1)
            },
            0,
        )
    })
}

/// 后端实测输出延迟（秒）。未初始化或样本不足时返回 0。
#[no_mangle]
pub extern "C" fn SasaUnity_EstimateLatency() -> c_float {
    MANAGER.with_borrow(|slot| slot.as_ref().map_or(0., |m| m.estimate_latency()))
}

// ============================================================
// AudioClip
// ============================================================

/// 解码压缩音频（mp3/aac/ogg/wav/flac）。失败返回 null。
///
/// SAFETY: `data` 必须指向至少 `size` 字节的可读内存。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_ClipDecode(data: *const u8, size: usize) -> Handle {
    clear_error();
    if data.is_null() {
        set_error("null data pointer");
        return null_mut();
    }

    let bytes = slice::from_raw_parts(data, size).to_vec();
    match AudioClip::new(bytes) {
        Ok(clip) => into_handle(clip),
        Err(e) => {
            set_error(e);
            null_mut()
        }
    }
}

/// 从 Unity `AudioClip.GetData` 的交错 PCM 建 clip。多于 2 声道时只取前两路。
///
/// SAFETY: `samples` 必须指向至少 `sample_count` 个 f32。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_ClipFromRaw(
    samples: *const c_float,
    sample_count: usize,
    channels: c_uint,
    sample_rate: c_uint,
) -> Handle {
    clear_error();
    if samples.is_null() {
        set_error("null samples pointer");
        return null_mut();
    }
    if channels == 0 || sample_rate == 0 {
        set_error("invalid channels or sample rate");
        return null_mut();
    }

    let data = slice::from_raw_parts(samples, sample_count);
    let channels = channels as usize;
    let frames: Vec<Frame> = data
        .chunks_exact(channels)
        .map(|frame| {
            if channels == 1 {
                Frame(frame[0], frame[0])
            } else {
                Frame(frame[0], frame[1])
            }
        })
        .collect();

    into_handle(AudioClip::from_raw(frames, sample_rate))
}

#[no_mangle]
pub unsafe extern "C" fn SasaUnity_ClipDestroy(handle: Handle) {
    clear_error();
    drop_handle::<AudioClip>(handle);
}

/// 时长（秒）。句柄无效时返回 0。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_ClipLength(handle: Handle) -> c_float {
    as_ref::<AudioClip>(handle).map_or(0., |clip| clip.length())
}

/// 把波形压成 `count` 个桶的峰值（每桶取声道绝对值的最大值）写入 `out`。
/// 练习模式的波形图用——改用 sasa 后没有 Unity AudioClip，`GetData` 也就没了。
///
/// SAFETY: `out` 必须指向至少 `count` 个可写 f32。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_ClipPeaks(
    handle: Handle,
    out: *mut c_float,
    count: usize,
) -> c_uchar {
    clear_error();
    if out.is_null() || count == 0 {
        set_error("invalid output buffer");
        return 0;
    }
    let Some(clip) = as_ref::<AudioClip>(handle) else {
        set_error("invalid clip handle");
        return 0;
    };

    let frames = clip.frames();
    let dst = slice::from_raw_parts_mut(out, count);

    if frames.is_empty() {
        dst.fill(0.);
        return 1;
    }

    // 用乘法定位桶边界而不是先算 per_bucket 再乘：帧数不是 count 整数倍时
    // 后者会漏掉尾部若干帧，波形右端出现一段假的静音。
    // 乘法走 u64：armeabi-v7a 上 usize 是 32 位，len * count 在几分钟长的曲子上
    // 就能越过 4.29e9，release 关溢出检查后会静默回绕，波形变成乱码。
    let len = frames.len() as u64;
    let count64 = count as u64;
    for (i, slot) in dst.iter_mut().enumerate() {
        let start = (len * i as u64 / count64) as usize;
        let end = ((len * (i as u64 + 1) / count64) as usize)
            .max(start + 1)
            .min(frames.len());
        let mut peak = 0f32;
        for frame in &frames[start..end] {
            peak = peak.max(frame.0.abs()).max(frame.1.abs());
        }
        *slot = peak;
    }
    1
}

// ============================================================
// Music
// ============================================================

/// 创建音乐轨。创建后处于暂停态，需显式 `Play`。
/// `loop_mix_time` 传负数表示不循环。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_MusicCreate(
    clip: Handle,
    amplifier: c_float,
    playback_rate: c_float,
    loop_mix_time: c_float,
) -> Handle {
    clear_error();
    let Some(clip) = as_ref::<AudioClip>(clip) else {
        set_error("invalid clip handle");
        return null_mut();
    };
    let clip = clip.clone();

    let params = MusicParams {
        amplifier,
        playback_rate,
        loop_mix_time,
        ..Default::default()
    };

    with_manager(null_mut(), |manager| match manager.create_music(clip, params) {
        Ok(music) => into_handle(music),
        Err(e) => {
            set_error(e);
            null_mut()
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn SasaUnity_MusicDestroy(handle: Handle) {
    clear_error();
    drop_handle::<Music>(handle);
}

macro_rules! music_op {
    ($name:ident, $method:ident $(, $arg:ident)?) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(handle: Handle $(, $arg: c_float)?) -> c_uchar {
            clear_error();
            match as_ref::<Music>(handle) {
                Some(music) => fallible(
                    || {
                        music.$method($($arg)?)?;
                        Ok(1)
                    },
                    0,
                ),
                None => {
                    set_error("invalid music handle");
                    0
                }
            }
        }
    };
}

music_op!(SasaUnity_MusicPlay, play);
music_op!(SasaUnity_MusicPause, pause);
music_op!(SasaUnity_MusicSeekTo, seek_to, position);
music_op!(SasaUnity_MusicSetAmplifier, set_amplifier, amplifier);
music_op!(SasaUnity_MusicSetLowPass, set_low_pass, low_pass);
music_op!(SasaUnity_MusicFadeIn, fade_in, time);
music_op!(SasaUnity_MusicFadeOut, fade_out, time);

/// 播放位置（秒）。这是音频线程回写的实际位置，用于校正墙钟。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_MusicPosition(handle: Handle) -> c_float {
    as_ref::<Music>(handle).map_or(0., |music| music.position())
}

#[no_mangle]
pub unsafe extern "C" fn SasaUnity_MusicIsPaused(handle: Handle) -> c_uchar {
    as_ref::<Music>(handle).map_or(1, |music| u8::from(music.paused()))
}

// ============================================================
// Sfx
// ============================================================

/// `buffer_size` 是同时可叠加播放的最大实例数，传 0 用 sasa 默认值（64）。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_SfxCreate(clip: Handle, buffer_size: c_uint) -> Handle {
    clear_error();
    let Some(clip) = as_ref::<AudioClip>(clip) else {
        set_error("invalid clip handle");
        return null_mut();
    };
    let clip = clip.clone();

    let size = if buffer_size == 0 {
        None
    } else {
        Some(buffer_size as usize)
    };

    with_manager(null_mut(), |manager| match manager.create_sfx(clip, size) {
        Ok(sfx) => into_handle(sfx),
        Err(e) => {
            set_error(e);
            null_mut()
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn SasaUnity_SfxDestroy(handle: Handle) {
    clear_error();
    drop_handle::<Sfx>(handle);
}

/// 触发一次播放。环形缓冲满时返回 0（打击音过密，丢弃这一次）。
#[no_mangle]
pub unsafe extern "C" fn SasaUnity_SfxPlay(handle: Handle, amplifier: c_float) -> c_uchar {
    clear_error();
    match as_ref::<Sfx>(handle) {
        Some(sfx) => fallible(
            || {
                sfx.play(PlaySfxParams { amplifier })?;
                Ok(1)
            },
            0,
        ),
        None => {
            set_error("invalid sfx handle");
            0
        }
    }
}
