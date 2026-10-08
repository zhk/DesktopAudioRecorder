//! WASAPI 采集，支持两种模式：
//!   `loopback = true`  —— 录制播放设备正在输出的声音（桌面音频回录）
//!   `loopback = false` —— 录制采集设备输入的声音（麦克风）
//!
//! 逻辑参考 OBS 的 `plugins/win-wasapi`，去掉对 libobs 的依赖；
//! 与 C++ 版 `src/wasapi_capture.{h,cpp}` 一一对应。
//!
//! 使用流程：
//! ```ignore
//! let mut cap = WasapiCapture::new();
//! cap.start(Some(&device_id), true)?;      // 阻塞直到设备初始化成功/失败
//! cap.begin(on_data, on_error);            // 真正开始采集，PCM16 从回调吐出
//! // ...
//! cap.stop();                              // 停止并回收线程
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use core::ffi::c_void;
use core::ptr::null_mut;

use crate::ffi::system::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CloseHandle, CoCreateInstance,
    CoTaskMemFree, ComApartment, CreateEventW, WaitForSingleObject, WAIT_OBJECT_0,
};
use crate::ffi::wasapi::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    CLSID_MMDeviceEnumerator, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, KSDATAFORMAT_SUBTYPE_PCM,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVE_FORMAT_EXTENSIBLE, WAVE_FORMAT_IEEE_FLOAT,
    WAVE_FORMAT_PCM, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_S_BUFFER_EMPTY,
};
use crate::ffi::{
    failed, hr_message as hr_msg, Com, ComPtr, DWORD, HANDLE, REFERENCE_TIME, CLSCTX_ALL, FALSE,
};

/// 交错存放的 PCM16 数据回调（`data.len()` 已含全部声道）。
pub type DataFn = Box<dyn FnMut(&[i16]) + Send + 'static>;
/// 运行期致命错误回调（如设备被拔出），在采集线程触发。
pub type ErrorFn = Box<dyn FnMut(String) + Send + 'static>;

/// `Begin()` 与采集线程之间的"发令枪"。
#[derive(PartialEq, Eq, Clone, Copy)]
enum Gate {
    Wait,
    Go,
    Abort,
}

struct Shared {
    running: AtomicBool,
    gate: Mutex<Gate>,
    gate_cv: Condvar,
    data_fn: Mutex<Option<DataFn>>,
    error_fn: Mutex<Option<ErrorFn>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(true),
            gate: Mutex::new(Gate::Wait),
            gate_cv: Condvar::new(),
            data_fn: Mutex::new(None),
            error_fn: Mutex::new(None),
        }
    }

    fn set_callbacks(&self, on_data: DataFn, on_error: ErrorFn) {
        *self.data_fn.lock().unwrap() = Some(on_data);
        *self.error_fn.lock().unwrap() = Some(on_error);
    }

    fn invoke_data(&self, data: &[i16]) {
        if let Some(cb) = self.data_fn.lock().unwrap().as_mut() {
            cb(data);
        }
    }

    fn invoke_error(&self, msg: String) {
        if let Some(cb) = self.error_fn.lock().unwrap().as_mut() {
            cb(msg);
        }
    }
}

/// 一路 WASAPI 采集（桌面 loopback 或麦克风）。
pub struct WasapiCapture {
    shared: Arc<Shared>,
    handle: Mutex<Option<JoinHandle<()>>>,
    rate: u32,
    channels: u16,
}

impl Default for WasapiCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl WasapiCapture {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::new()),
            handle: Mutex::new(None),
            rate: 0,
            channels: 0,
        }
    }

    /// 启动采集线程并初始化设备，阻塞直到初始化完成。
    ///
    /// `device_id` 为 `None` 时使用默认设备；`loopback` 选择录制方向。
    ///
    /// # Errors
    /// 设备打开/激活/格式协商失败时返回可读的错误描述。
    pub fn start(&mut self, device_id: Option<&[u16]>, loopback: bool) -> Result<(), String> {
        self.stop();

        let shared = Arc::new(Shared::new());
        let (tx, rx) = mpsc::sync_channel::<Result<(u32, u16), String>>(1);
        let thread_shared = Arc::clone(&shared);
        let device = device_id.map(<[u16]>::to_vec);
        let name = if loopback { "wasapi-loopback" } else { "wasapi-mic" };

        let handle = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || capture_thread(thread_shared, device, loopback, tx))
            .map_err(|e| format!("创建采集线程失败：{e}"))?;

        match rx.recv() {
            Ok(Ok((rate, channels))) => {
                self.rate = rate;
                self.channels = channels;
                self.shared = shared;
                *self.handle.lock().unwrap() = Some(handle);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err("采集线程在初始化阶段异常退出".to_string())
            }
        }
    }

    /// 设置回调并让采集线程真正开始工作（`start` 成功后调用）。
    pub fn begin(&mut self, on_data: DataFn, on_error: ErrorFn) {
        self.shared.set_callbacks(on_data, on_error);
        {
            let mut gate = self.shared.gate.lock().unwrap();
            *gate = Gate::Go;
        }
        self.shared.gate_cv.notify_all();
    }

    /// 停止采集并回收线程，可重复调用。
    pub fn stop(&self) {
        self.shared.running.store(false, Ordering::SeqCst);
        {
            let mut gate = self.shared.gate.lock().unwrap();
            if *gate == Gate::Wait {
                // 若还停在 Begin 之前，放行线程退出
                *gate = Gate::Abort;
            }
        }
        self.shared.gate_cv.notify_all();

        let handle = self.handle.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// 输出声道数：设备多于 2 声道时会降混为立体声。
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl Drop for WasapiCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 设备初始化阶段持有的 COM 资源；`Drop` 负责释放，错误路径同样受益。
struct Ctx {
    client: ComPtr<IAudioClient>,
    capture: ComPtr<IAudioCaptureClient>,
    /// `GetMixFormat` 用 `CoTaskMemAlloc` 分配，必须 `CoTaskMemFree`
    wfx: *mut WAVEFORMATEX,
    ev_data: HANDLE,
    is_float: bool,
    in_channels: u16,
    out_channels: u16,
    rate: u32,
}

impl Drop for Ctx {
    fn drop(&mut self) {
        unsafe {
            if !self.wfx.is_null() {
                CoTaskMemFree(self.wfx as *mut c_void);
                self.wfx = null_mut();
            }
            if !self.ev_data.is_null() {
                CloseHandle(self.ev_data);
                self.ev_data = null_mut();
            }
        }
    }
}

impl Ctx {
    /// 打开设备并完成格式协商；成功后设备已 `Initialize`，但尚未 `Start`。
    ///
    /// # Safety
    /// 调用线程必须已初始化 COM（MTA）。
    unsafe fn open(device_id: Option<&[u16]>, loopback: bool) -> Result<Self, String> {
        let mut enumerator = ComPtr::<IMMDeviceEnumerator>::null();
        let hr = CoCreateInstance(
            &CLSID_MMDeviceEnumerator,
            null_mut(),
            CLSCTX_ALL,
            &IMMDeviceEnumerator::IID,
            enumerator.as_void_out_ptr(),
        );
        if failed(hr) {
            return Err(hr_msg("创建 MMDeviceEnumerator 失败", hr));
        }

        let mut device = ComPtr::<IMMDevice>::null();
        let hr = match device_id {
            None => enumerator.expect_ref().get_default_audio_endpoint(
                if loopback { eRender } else { eCapture },
                eConsole,
                device.as_out_ptr(),
            ),
            Some(id) => enumerator.expect_ref().get_device(id.as_ptr(), device.as_out_ptr()),
        };
        if failed(hr) {
            let what = if loopback { "播放" } else { "录音" };
            return Err(hr_msg(&format!("打开{what}设备失败"), hr));
        }

        let mut client = ComPtr::<IAudioClient>::null();
        let hr = device.expect_ref().activate(
            &IAudioClient::IID,
            CLSCTX_ALL,
            null_mut(),
            client.as_void_out_ptr(),
        );
        if failed(hr) {
            return Err(hr_msg("激活 IAudioClient 失败", hr));
        }

        let mut wfx: *mut WAVEFORMATEX = null_mut();
        let hr = client.expect_ref().get_mix_format(&mut wfx);
        if failed(hr) {
            return Err(hr_msg("GetMixFormat 失败", hr));
        }

        // 从这里开始任何错误路径都会 drop(ctx)，从而释放 wfx
        let mut ctx = Ctx {
            client,
            capture: ComPtr::null(),
            wfx,
            ev_data: null_mut(),
            is_float: false,
            in_channels: 0,
            out_channels: 0,
            rate: 0,
        };

        // 只接受 float32 / int16 的混音格式（现代 Windows 上几乎总是 float32）
        let w = ctx.wfx;
        let tag = (*w).wFormatTag;
        let bits = (*w).wBitsPerSample;
        if tag == WAVE_FORMAT_IEEE_FLOAT && bits == 32 {
            ctx.is_float = true;
        } else if tag == WAVE_FORMAT_PCM && bits == 16 {
            ctx.is_float = false;
        } else if tag == WAVE_FORMAT_EXTENSIBLE && (*w).cbSize >= 22 {
            let ext = w as *const WAVEFORMATEXTENSIBLE;
            let sub = (*ext).SubFormat;
            if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && bits == 32 {
                ctx.is_float = true;
            } else if sub == KSDATAFORMAT_SUBTYPE_PCM && bits == 16 {
                ctx.is_float = false;
            } else {
                return Err("不支持的混音格式（仅支持 float32 / int16）".to_string());
            }
        } else {
            return Err("不支持的混音格式（仅支持 float32 / int16）".to_string());
        }

        ctx.ev_data = CreateEventW(null_mut(), FALSE, FALSE, core::ptr::null());
        if ctx.ev_data.is_null() {
            return Err("CreateEvent 失败".to_string());
        }

        // 100ms 缓冲区 + 事件驱动，与 OBS win-wasapi 的事件模式一致
        let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        if loopback {
            flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }
        let buffer_duration: REFERENCE_TIME = 100 * 10_000;
        let hr = ctx.client.expect_ref().initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            buffer_duration,
            0,
            ctx.wfx,
            core::ptr::null(),
        );
        if failed(hr) {
            return Err(hr_msg("IAudioClient::Initialize 失败", hr));
        }

        let hr = ctx.client.expect_ref().set_event_handle(ctx.ev_data);
        if failed(hr) {
            return Err(hr_msg("SetEventHandle 失败", hr));
        }

        let hr = ctx
            .client
            .expect_ref()
            .get_service(&IAudioCaptureClient::IID, ctx.capture.as_void_out_ptr());
        if failed(hr) {
            return Err(hr_msg("获取 IAudioCaptureClient 失败", hr));
        }

        ctx.rate = (*w).nSamplesPerSec;
        ctx.in_channels = (*w).nChannels;
        ctx.out_channels = if ctx.in_channels > 2 { 2 } else { ctx.in_channels };
        Ok(ctx)
    }
}

/// 采集线程主体：初始化设备 -> 等 `begin()` 发令 -> 循环取包。
fn capture_thread(
    shared: Arc<Shared>,
    device_id: Option<Vec<u16>>,
    loopback: bool,
    init: mpsc::SyncSender<Result<(u32, u16), String>>,
) {
    unsafe {
        let _com = ComApartment::mta();

        let task_name = crate::util::to_wide("Pro Audio");
        let mut task_index: DWORD = 0;
        let mmcss = AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index);

        let mut ctx = match Ctx::open(device_id.as_deref(), loopback) {
            Ok(c) => c,
            Err(e) => {
                let _ = init.send(Err(e));
                if !mmcss.is_null() {
                    AvRevertMmThreadCharacteristics(mmcss);
                }
                return;
            }
        };
        let _ = init.send(Ok((ctx.rate, ctx.out_channels)));

        // 等 Begin()；Begin() 之前就被 Stop() 则直接退出
        let go = {
            let mut gate = shared.gate.lock().unwrap();
            while *gate == Gate::Wait && shared.running.load(Ordering::SeqCst) {
                gate = shared.gate_cv.wait(gate).unwrap();
            }
            *gate
        };

        let mut fatal = None;
        if go == Gate::Go && shared.running.load(Ordering::SeqCst) {
            fatal = run_capture(&shared, &mut ctx);
        }

        drop(ctx);
        if !mmcss.is_null() {
            AvRevertMmThreadCharacteristics(mmcss);
        }
        if let Some(msg) = fatal {
            shared.invoke_error(msg);
        }
    }
}

/// 采集主循环。返回致命错误描述（正常停止时为 `None`）。
///
/// # Safety
/// `ctx` 必须已完成 `Initialize` 且线程已初始化 COM。
unsafe fn run_capture(shared: &Shared, ctx: &mut Ctx) -> Option<String> {
    let mut fatal: Option<String> = None;

    let hr = ctx.client.expect_ref().start();
    if failed(hr) {
        fatal = Some(hr_msg("IAudioClient::Start 失败", hr));
    }

    // 复用同一块缓冲，避免每个数据包都分配
    let mut chunk: Vec<i16> = Vec::new();

    while shared.running.load(Ordering::SeqCst) && fatal.is_none() {
        if WaitForSingleObject(ctx.ev_data, 500) != WAIT_OBJECT_0 {
            continue;
        }

        let capture = ctx.capture.expect_ref();
        let mut packet_frames: u32 = 0;
        capture.get_next_packet_size(&mut packet_frames);
        while packet_frames > 0 {
            let mut data: *mut u8 = null_mut();
            let mut frames: u32 = 0;
            let mut flags: DWORD = 0;
            let hr = capture.get_buffer(&mut data, &mut frames, &mut flags, null_mut(), null_mut());
            if hr == AUDCLNT_S_BUFFER_EMPTY {
                break;
            }
            if failed(hr) {
                fatal = Some(if hr == AUDCLNT_E_DEVICE_INVALIDATED {
                    "音频设备已断开或失效".to_string()
                } else {
                    hr_msg("GetBuffer 失败", hr)
                });
                break;
            }

            if frames > 0 {
                convert_packet(ctx, data, frames, flags, &mut chunk);
                shared.invoke_data(&chunk);
            }

            capture.release_buffer(frames);
            capture.get_next_packet_size(&mut packet_frames);
        }
    }

    ctx.client.expect_ref().stop();
    fatal
}

/// 把驱动给的一包数据转成交错 PCM16（必要时降混为立体声）。
///
/// # Safety
/// `data` 必须是 `frames * in_channels` 个样本的有效缓冲，且格式与 `ctx` 一致。
///
// 下面刻意使用与 C++ 版相同的十进制字面量（0.7071 / 0.3536）而不是
// std::f32::consts::FRAC_1_SQRT_2，以保持逐样本输出完全一致。
#[allow(clippy::approx_constant)]
unsafe fn convert_packet(ctx: &Ctx, data: *const u8, frames: u32, flags: DWORD, chunk: &mut Vec<i16>) {
    let in_ch = ctx.in_channels as usize;
    let out_ch = ctx.out_channels as usize;

    chunk.clear();
    chunk.resize(frames as usize * out_ch, 0);
    if flags & AUDCLNT_BUFFERFLAGS_SILENT != 0 {
        // 静音包：保持全 0，仍然交给回调（保持时间轴连续）
        return;
    }

    let src_f = data as *const f32;
    let src_i = data as *const i16;
    let is_float = ctx.is_float;
    let sample_at = |idx: usize| -> f32 {
        if is_float {
            *src_f.add(idx)
        } else {
            *src_i.add(idx) as f32 / 32768.0
        }
    };

    for f in 0..frames as usize {
        let base = f * in_ch;
        let out = &mut chunk[f * out_ch..f * out_ch + out_ch];
        if in_ch == out_ch {
            for (ch, slot) in out.iter_mut().enumerate() {
                *slot = float_to_i16(sample_at(base + ch));
            }
        } else {
            // 多声道降混为立体声：
            // 前置 L/R + 0.707*中置 + 环绕均摊，丢弃 LFE
            let mut l = sample_at(base);
            let mut r = sample_at(base + 1);
            if in_ch >= 3 {
                let ctr = sample_at(base + 2);
                l += 0.7071 * ctr;
                r += 0.7071 * ctr;
            }
            for ch in 4..in_ch {
                let s = sample_at(base + ch);
                l += 0.3536 * s;
                r += 0.3536 * s;
            }
            out[0] = float_to_i16(l);
            out[1] = float_to_i16(r);
        }
    }
}

/// 与 C++ 版的 `FloatToI16` 一致：先限幅到 [-1, 1]，再乘 32767 并四舍五入。
fn float_to_i16(v: f32) -> i16 {
    let v = v.clamp(-1.0, 1.0);
    // C 的 lrintf 默认使用"就近取偶"舍入，round_ties_even 与之等价
    (v * 32767.0).round_ties_even() as i16
}
