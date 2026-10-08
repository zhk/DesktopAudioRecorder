//! 冒烟自检：直接在真实系统上跑一遍关键路径，验证手写 FFI 的 vtable 顺序、
//! 结构体布局与 GUID 常量都没写错。不弹任何窗口。
//!
//! 用法（在 src_rs 目录下）：
//! ```text
//! cargo run --example smoke
//! ```
//!
//! 覆盖：
//!   1. MMDevice API 设备枚举（`IMMDeviceEnumerator` / `IMMDeviceCollection` /
//!      `IMMDevice` / `IPropertyStore` / `PROPVARIANT`）；
//!   2. 会话枚举（`IAudioSessionManager2` / `IAudioSessionEnumerator` /
//!      `IAudioSessionControl` / `IAudioSessionControl2`），即 Teams 看门狗的路径；
//!   3. Media Foundation MP3 编码（`IMFSinkWriter` / `IMFMediaType` /
//!      `IMFMediaSample` / `IMFMediaBuffer`）：把 1 秒 440Hz 正弦写成 mp3 并校验文件头；
//!   4. 混音器 + 回声消除的数值健全性（含一次采集回调路径的等价复现）。

use std::path::PathBuf;
use std::ptr::null_mut;

use desktop_audio_recorder::audio_mixer::AudioMixer;
use desktop_audio_recorder::echo_canceller::EchoCanceller;
use desktop_audio_recorder::ffi::system::{
    CoCreateInstance, CoTaskMemFree, ComApartment, PropVariantClear,
};
use desktop_audio_recorder::ffi::wasapi::{
    eCapture, eConsole, eRender, EDataFlow, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice, IMMDeviceCollection,
    IMMDeviceEnumerator, IPropertyStore, CLSID_MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    PKEY_DEVICE_FRIENDLY_NAME, STGM_READ,
};
use desktop_audio_recorder::ffi::{
    failed, succeeded, Com, ComPtr, LPWSTR, PROPVARIANT, VT_LPWSTR,
};
use desktop_audio_recorder::mp3_writer::Mp3Writer;
use desktop_audio_recorder::teams_watcher::TeamsWatcher;
use desktop_audio_recorder::util::from_wide;

fn main() {
    // Safety: 单线程自检，COM 初始化与反初始化配对。
    let _com = unsafe { ComApartment::sta() };

    let mut failures = 0usize;
    failures += run("设备枚举（MMDevice API）", check_enumerate_devices);
    failures += run("音频会话枚举（Teams 看门狗路径）", check_session_enumeration);
    failures += run("MP3 编码（Media Foundation Sink Writer）", check_mp3_encoding);
    failures += run("实时 loopback 采集 -> 混音 -> MP3 全链路", check_loopback_capture);
    failures += run("混音器 / 回声消除数值健全性", check_mixer_and_aec);

    println!();
    if failures == 0 {
        println!("全部通过 ✅");
    } else {
        println!("{failures} 项失败 ❌");
        std::process::exit(1);
    }
}

/// 跑一个检查项，打印结果并返回失败计数（0 或 1）。
fn run(name: &str, check: fn() -> Result<(), String>) -> usize {
    println!("== {name}");
    match check() {
        Ok(()) => {
            println!("   PASS");
            0
        }
        Err(e) => {
            println!("   FAIL: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// 1) 设备枚举
// ---------------------------------------------------------------------------

fn check_enumerate_devices() -> Result<(), String> {
    let render = enumerate(eRender)?;
    for d in &render {
        println!("   播放设备: {}  ({})", d.name, d.id_preview);
    }
    let capture = enumerate(eCapture)?;
    for d in &capture {
        println!("   录音设备: {}  ({})", d.name, d.id_preview);
    }
    if render.is_empty() {
        println!("   注意：当前系统没有活动的播放设备（自检仍算通过）");
    }
    Ok(())
}

struct DeviceInfo {
    name: String,
    id_preview: String,
}

/// 列出某个数据流方向的全部活动端点。
fn enumerate(flow: EDataFlow) -> Result<Vec<DeviceInfo>, String> {
    // Safety: 每个 COM 对象都由 ComPtr 管理生命周期。
    unsafe {
        let mut enumerator = ComPtr::<IMMDeviceEnumerator>::null();
        let hr = CoCreateInstance(
            &CLSID_MMDeviceEnumerator,
            null_mut(),
            desktop_audio_recorder::ffi::CLSCTX_ALL,
            &IMMDeviceEnumerator::IID,
            enumerator.as_void_out_ptr(),
        );
        if failed(hr) {
            return Err(format!("CoCreateInstance(MMDeviceEnumerator) 失败: {hr:#010X}"));
        }

        let mut collection = ComPtr::<IMMDeviceCollection>::null();
        let hr = enumerator
            .expect_ref()
            .enum_audio_endpoints(flow, DEVICE_STATE_ACTIVE, collection.as_out_ptr());
        if failed(hr) {
            return Err(format!("EnumAudioEndpoints 失败: {hr:#010X}"));
        }

        let mut count: u32 = 0;
        let hr = collection.expect_ref().get_count(&mut count);
        if failed(hr) {
            return Err(format!("GetCount 失败: {hr:#010X}"));
        }

        let mut out = Vec::new();
        for i in 0..count {
            let mut device = ComPtr::<IMMDevice>::null();
            if failed(collection.expect_ref().item(i, device.as_out_ptr())) {
                continue;
            }
            let device = device.expect_ref();

            let mut id_ptr: LPWSTR = null_mut();
            let id = if succeeded(device.get_id(&mut id_ptr)) && !id_ptr.is_null() {
                let s = from_wide(wide_slice(id_ptr));
                CoTaskMemFree(id_ptr as *mut core::ffi::c_void);
                s
            } else {
                String::new()
            };

            let mut name = String::new();
            let mut store = ComPtr::<IPropertyStore>::null();
            if succeeded(device.open_property_store(STGM_READ, store.as_out_ptr())) {
                let mut pv = PROPVARIANT::zeroed();
                if succeeded(store.expect_ref().get_value(&PKEY_DEVICE_FRIENDLY_NAME, &mut pv))
                    && pv.vt == VT_LPWSTR
                    && !pv.data.pwsz_val.is_null()
                {
                    name = from_wide(wide_slice(pv.data.pwsz_val));
                }
                PropVariantClear(&mut pv);
            }

            // 设备 ID 很长，这里只留前 24 个字符方便肉眼比对
            let id_preview = if id.chars().count() > 24 {
                format!("{}…", id.chars().take(24).collect::<String>())
            } else {
                id.clone()
            };
            out.push(DeviceInfo { name, id_preview });
        }
        Ok(out)
    }
}

/// 读取 NUL 结尾的 UTF-16 字符串（不含结尾 NUL）。
///
/// # Safety
/// `p` 必须指向合法的 NUL 结尾 UTF-16 字符串。
unsafe fn wide_slice(p: *const u16) -> &'static [u16] {
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::slice::from_raw_parts(p, len)
}

// ---------------------------------------------------------------------------
// 2) 会话枚举（Teams 看门狗走的就是这条路径）
// ---------------------------------------------------------------------------

fn check_session_enumeration() -> Result<(), String> {
    // Safety: 每个 COM 对象都由 ComPtr 管理生命周期。
    unsafe {
        let mut enumerator = ComPtr::<IMMDeviceEnumerator>::null();
        if failed(CoCreateInstance(
            &CLSID_MMDeviceEnumerator,
            null_mut(),
            desktop_audio_recorder::ffi::CLSCTX_ALL,
            &IMMDeviceEnumerator::IID,
            enumerator.as_void_out_ptr(),
        )) {
            return Err("创建 MMDeviceEnumerator 失败".to_string());
        }

        let mut device = ComPtr::<IMMDevice>::null();
        if failed(enumerator.expect_ref().get_default_audio_endpoint(
            eRender,
            eConsole,
            device.as_out_ptr(),
        )) {
            println!("   当前没有默认播放设备，跳过会话枚举");
            return Ok(());
        }

        let mut manager = ComPtr::<IAudioSessionManager2>::null();
        if failed(device.expect_ref().activate(
            &IAudioSessionManager2::IID,
            desktop_audio_recorder::ffi::CLSCTX_ALL,
            null_mut(),
            manager.as_void_out_ptr(),
        )) {
            return Err("Activate(IAudioSessionManager2) 失败".to_string());
        }

        let mut sessions = ComPtr::<IAudioSessionEnumerator>::null();
        if failed(manager.expect_ref().get_session_enumerator(sessions.as_out_ptr())) {
            return Err("GetSessionEnumerator 失败".to_string());
        }

        let mut count: i32 = 0;
        if failed(sessions.expect_ref().get_count(&mut count)) {
            return Err("IAudioSessionEnumerator::GetCount 失败".to_string());
        }
        println!("   当前播放设备上的音频会话数: {count}");

        for index in 0..count {
            let mut control = ComPtr::<IAudioSessionControl>::null();
            if failed(sessions.expect_ref().get_session(index, control.as_out_ptr())) {
                continue;
            }
            let mut state: i32 = 0;
            control.expect_ref().get_state(&mut state);

            let mut pid: u32 = 0;
            if let Ok(control2) = control.query::<IAudioSessionControl2>() {
                control2.expect_ref().get_process_id(&mut pid);
            }
            println!("   会话 #{index}: state={state} pid={pid}");
        }

        // Teams 看门狗自身的调用路径（当前多半没有会议，返回 false 也算正常）
        println!("   TeamsWatcher::meeting_active() = {}", TeamsWatcher::meeting_active());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3) MP3 编码
// ---------------------------------------------------------------------------

fn check_mp3_encoding() -> Result<(), String> {
    const RATE: u32 = 48_000;
    const CHANNELS: u16 = 2;
    const SECONDS: usize = 1;

    let path: PathBuf = std::env::temp_dir().join("desktop-audio-recorder-smoke.mp3");
    let _ = std::fs::remove_file(&path);

    let writer = Mp3Writer::open(&path, RATE, CHANNELS, 192_000)?;

    // 440Hz 立体声正弦，按 10ms 一包喂进去（与真实采集的包大小一致）
    let frames_per_chunk = RATE as usize / 100;
    let chunks = RATE as usize * SECONDS / frames_per_chunk;
    let mut phase = 0.0f64;
    let step = 2.0 * std::f64::consts::PI * 440.0 / RATE as f64;

    for _ in 0..chunks {
        let mut chunk = Vec::with_capacity(frames_per_chunk * CHANNELS as usize);
        for _ in 0..frames_per_chunk {
            let v = (phase.sin() * 0.5 * i16::MAX as f64) as i16;
            phase += step;
            chunk.push(v);
            chunk.push(v);
        }
        writer.push(&chunk);
    }
    writer.close();

    if writer.has_error() {
        return Err(format!("编码失败: {}", writer.error()));
    }

    let meta = std::fs::metadata(&path).map_err(|e| format!("输出文件不存在: {e}"))?;
    println!("   输出: {} ({} 字节)", path.display(), meta.len());
    // 192kbps 单声道/立体声 1 秒 → 约 24KB；给一个宽松下限
    if meta.len() < 4_000 {
        return Err(format!("MP3 文件过小，疑似编码器没工作: {} 字节", meta.len()));
    }

    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    // 校验前若干字节里存在 MP3 帧同步字（0xFF 0xE0 掩码）
    let has_sync = bytes
        .windows(2)
        .take(4096)
        .any(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0);
    if !has_sync {
        return Err("前 4KB 内找不到 MP3 帧同步字".to_string());
    }
    println!("   找到 MP3 帧同步字，编码链路正常");

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// 4) 实时 loopback 采集 -> 混音 -> MP3（与 GUI 里的接线方式完全相同）
// ---------------------------------------------------------------------------

fn check_loopback_capture() -> Result<(), String> {
    use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
    use std::sync::Mutex;

    use desktop_audio_recorder::wasapi_capture::WasapiCapture;

    const SECONDS: u64 = 3;

    let mut desktop = WasapiCapture::new();
    desktop
        .start(None, true)
        .map_err(|e| format!("打开默认播放设备（loopback）失败: {e}"))?;
    let rate = desktop.sample_rate();
    let channels = desktop.channels();
    println!("   loopback 格式: {rate} Hz, {channels} ch");

    let path: PathBuf = std::env::temp_dir().join("desktop-audio-recorder-smoke-capture.mp3");
    let _ = std::fs::remove_file(&path);

    let writer = std::sync::Arc::new(Mp3Writer::open(&path, rate, channels, 192_000)?);
    let mixer = std::sync::Arc::new(AudioMixer::new());
    {
        let w = std::sync::Arc::clone(&writer);
        mixer.open(rate, channels, Box::new(move |data: &[i16]| w.push(data)));
    }

    let frames = std::sync::Arc::new(AtomicU64::new(0));
    let peak = std::sync::Arc::new(AtomicI32::new(0));
    let error: std::sync::Arc<Mutex<Option<String>>> = std::sync::Arc::new(Mutex::new(None));

    let mix = std::sync::Arc::clone(&mixer);
    let frame_counter = std::sync::Arc::clone(&frames);
    let peak_holder = std::sync::Arc::clone(&peak);
    let error_slot = std::sync::Arc::clone(&error);
    desktop.begin(
        Box::new(move |data: &[i16]| {
            frame_counter.fetch_add((data.len() / channels as usize) as u64, Ordering::Relaxed);
            let mut hi = peak_holder.load(Ordering::Relaxed);
            for &v in data {
                let a = (v as i32).abs();
                if a > hi {
                    hi = a;
                }
            }
            peak_holder.store(hi, Ordering::Relaxed);
            mix.push_desktop(data);
        }),
        Box::new(move |msg: String| {
            *error_slot.lock().unwrap() = Some(msg);
        }),
    );

    std::thread::sleep(std::time::Duration::from_secs(SECONDS));
    desktop.stop();
    writer.close();

    if let Some(msg) = error.lock().unwrap().clone() {
        return Err(format!("采集过程报错: {msg}"));
    }

    let frames = frames.load(Ordering::Relaxed);
    let peak = peak.load(Ordering::Relaxed);
    println!(
        "   {SECONDS} 秒内采集到 {frames} 帧（约 {:.2}s 音频），峰值振幅 {peak}",
        frames as f64 / rate as f64
    );

    if frames == 0 {
        // loopback 只在默认播放设备确实有音频输出时才有数据；此时编码器也会
        // （正确地）报告"未捕获到任何音频数据"并删掉空文件 —— 这是预期行为。
        println!("   注意：这段时间没有任何程序在播放声音，因此抓不到数据（loopback 的正常行为）");
        println!("   本次仅验证了设备打开/初始化/停止链路，自检仍算通过");
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }

    if writer.has_error() {
        return Err(format!("编码失败: {}", writer.error()));
    }

    let meta = std::fs::metadata(&path).map_err(|e| format!("采集成功但 MP3 未生成: {e}"))?;
    println!("   采集 -> 混音 -> MP3：{} 字节", meta.len());
    if peak <= 0 {
        println!("   注意：峰值振幅为 0，说明抓到的是静音数据");
    }
    if meta.len() < 2_000 {
        return Err(format!("MP3 文件过小，疑似编码未真正写入: {} 字节", meta.len()));
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// 5) 混音器 + AEC 数值健全性
// ---------------------------------------------------------------------------

fn check_mixer_and_aec() -> Result<(), String> {
    // ---- 混音器：桌面直通 + 麦克风混入 ----
    let out: std::sync::Arc<std::sync::Mutex<Vec<i16>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&out);
    let mixer = AudioMixer::new();
    mixer.open(48_000, 2, Box::new(move |data: &[i16]| sink.lock().unwrap().extend_from_slice(data)));

    // 不接麦克风时应当直通
    let desktop: Vec<i16> = (0..480).map(|i| (i % 100) as i16 * 10).collect();
    mixer.push_desktop(&desktop);
    {
        let got = out.lock().unwrap();
        if got.len() != desktop.len() || got[..] != desktop[..] {
            return Err("未启用麦克风时桌面流应当直通，但输出不一致".to_string());
        }
    }
    out.lock().unwrap().clear();

    // 接入麦克风（同速率、单声道 -> 立体声），桌面 + 麦克风应逐样本相加
    mixer.set_mic_format(48_000, 1);
    out.lock().unwrap().clear();
    let mic: Vec<i16> = vec![1000; 240]; // 240 帧单声道
    mixer.push_mic(&mic);
    let desktop2: Vec<i16> = vec![100; 480];
    mixer.push_desktop(&desktop2);
    {
        let got = out.lock().unwrap();
        if got.len() != 480 {
            return Err(format!("混音输出长度应为 480，实际 {}", got.len()));
        }
        if got[0] != 1100 || got[1] != 1100 {
            return Err(format!("混音结果应为 100+1000=1100，实际 {} / {}", got[0], got[1]));
        }
    }

    // 削波保护：桌面 30000 + 麦克风 30000 应饱和到 32767
    out.lock().unwrap().clear();
    mixer.push_mic(&vec![30_000i16; 240]);
    mixer.push_desktop(&vec![30_000i16; 480]);
    {
        let got = out.lock().unwrap();
        if got[0] != 32_767 {
            return Err(format!("饱和防削波应为 32767，实际 {}", got[0]));
        }
    }
    println!("   混音器：直通 / 相加 / 饱和防削波 均正确");

    // ---- 回声消除：模拟"远端参考 -> 衰减延迟 -> 麦克风"，看残差能量是否下降 ----
    let mut aec = EchoCanceller::new();
    aec.configure(48_000, 1);
    let frames = 96_000usize; // 2 秒

    // 参考信号：白噪声（用确定性伪随机，避免依赖 rand）
    let mut seed = 0x1234_5678u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let reference: Vec<i16> = (0..frames).map(|_| (next() * 8000.0) as i16).collect();

    // 麦克风 = 参考的 0.2 倍延迟 64 个样本（模拟扬声器 -> 房间 -> 麦克风的回声路径）。
    // 增益刻意取到 -14dB：功率比 0.04，明显低于双讲检测阈值 0.25，
    // 因此不会触发"冻结系数更新"（真实回声路径也是这个量级）。
    const DELAY: usize = 64;
    const GAIN: f32 = 0.2;
    let mic_clean: Vec<i16> = (0..frames)
        .map(|i| {
            if i >= DELAY {
                (reference[i - DELAY] as f32 * GAIN) as i16
            } else {
                0
            }
        })
        .collect();

    // 参考与麦克风必须**交替**按 10ms 包喂进去：参考 FIFO 有 250ms 上限，
    // 一次性灌完会触发丢旧数据，和真实采集路径的行为不一致。
    let mut mic = mic_clean.clone();
    let chunk_frames = 480usize; // 10ms @ 48kHz
    for start in (0..frames).step_by(chunk_frames) {
        let end = (start + chunk_frames).min(frames);
        aec.push_reference(&reference[start..end], (end - start) as u32, 1);
        let mut mic_chunk = mic_clean[start..end].to_vec();
        aec.process(&mut mic_chunk);
        mic[start..end].copy_from_slice(&mic_chunk);
    }

    let energy = |data: &[i16], from: usize| -> f64 {
        data[from..].iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>()
    };
    let db = |before: f64, after: f64| 10.0 * (before / after.max(1.0)).log10();

    // 只看最后 0.5 秒（滤波器已收敛）
    let measure_from = frames - 24_000;
    let reduction_db = db(energy(&mic_clean, measure_from), energy(&mic, measure_from));
    // 顺带报告整个 2 秒的平均抑制，便于观察收敛过程
    let overall_db = db(energy(&mic_clean, 0), energy(&mic, 0));
    println!("   回声消除：收敛后（最后 0.5s）残差下降 {reduction_db:.1} dB，全程平均 {overall_db:.1} dB");
    if reduction_db < 10.0 {
        return Err(format!("回声抑制不足: {reduction_db:.1} dB"));
    }
    Ok(())
}
