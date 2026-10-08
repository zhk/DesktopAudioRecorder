//! Media Foundation Sink Writer 封装：PCM16 -> MP3 文件。
//! 与 C++ 版 `src/mp3_writer.{h,cpp}` 一一对应。
//!
//! 写入在独立线程进行，`push()` 只做入队，采集线程不会被编码阻塞。
//!
//! 与 C++ 版的一个实现差异：C++ 在主线程里创建 sink writer、在写线程里使用它；
//! Rust 这边把**所有** Media Foundation 调用都放进写线程（线程通过 channel 把
//! 初始化结果回传），这样 COM 对象始终不出现在 `Send` 边界上，无需 `unsafe impl Send`。
//!
//! 使用流程：
//! ```ignore
//! let w = Mp3Writer::open(path, 48000, 2, 192_000)?;
//! w.push(data);   // 可来自其他线程
//! w.close();      // 冲刷队列并 Finalize
//! ```

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use core::ptr::null_mut;

use crate::ffi::mf::{
    MFAudioFormat_MP3, MFAudioFormat_PCM, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFShutdown, MFStartup, IMFMediaBuffer,
    IMFMediaType, IMFSample, IMFSinkWriter, MF_MT_ALL_SAMPLES_INDEPENDENT,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AUDIO_VALID_BITS_PER_SAMPLE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFSTARTUP_FULL, MF_VERSION,
};
use crate::ffi::system::{ComApartment, DeleteFileW};
use crate::ffi::{failed, hr_message as hr_msg, ComPtr, DWORD};
use crate::util::path_to_wide;

/// 队列上限：约 5 秒（10ms 一包计）积压后丢包，避免内存膨胀。
const MAX_QUEUED_CHUNKS: usize = 500;

struct Queue {
    chunks: VecDeque<Vec<i16>>,
    /// 写线程是否继续消费；`close()` 置 false。
    running: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    failed: AtomicBool,
    error: Mutex<String>,
    open: AtomicBool,
}

/// Media Foundation MP3 编码器（写入队列 + 后台线程）。
pub struct Mp3Writer {
    shared: Arc<Shared>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Mp3Writer {
    /// 打开输出文件并完成编码器初始化，阻塞直到初始化成功/失败。
    ///
    /// # Errors
    /// `MFStartup` / 输出文件创建 / 编码器格式协商失败时返回可读描述。
    pub fn open(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bitrate_bps: u32,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue { chunks: VecDeque::new(), running: true }),
            cv: Condvar::new(),
            failed: AtomicBool::new(false),
            error: Mutex::new(String::new()),
            open: AtomicBool::new(false),
        });

        let (tx, rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let thread_shared = Arc::clone(&shared);
        let wide_path = path_to_wide(path);

        let handle = thread::Builder::new()
            .name("mp3-writer".to_string())
            .spawn(move || {
                writer_thread(thread_shared, wide_path, sample_rate, channels, bitrate_bps, tx)
            })
            .map_err(|e| format!("创建 MP3 编码线程失败：{e}"))?;

        match rx.recv() {
            Ok(Ok(())) => {
                shared.open.store(true, Ordering::SeqCst);
                Ok(Self { shared, handle: Mutex::new(Some(handle)) })
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err("MP3 编码线程在初始化阶段异常退出".to_string())
            }
        }
    }

    /// 入队一块交错 PCM16（可来自采集线程）。
    pub fn push(&self, data: &[i16]) {
        if !self.shared.open.load(Ordering::SeqCst) || self.shared.failed.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut queue = self.shared.queue.lock().unwrap();
            // 背压保护：积压过多直接丢弃
            if queue.chunks.len() > MAX_QUEUED_CHUNKS {
                return;
            }
            queue.chunks.push_back(data.to_vec());
        }
        self.shared.cv.notify_one();
    }

    /// 冲刷队列、`Finalize` 并回收写线程；可重复调用。
    pub fn close(&self) {
        {
            let mut queue = self.shared.queue.lock().unwrap();
            queue.running = false;
        }
        self.shared.cv.notify_all();

        let handle = self.handle.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
        self.shared.open.store(false, Ordering::SeqCst);
    }

    /// 录制过程中是否出过错（编码失败 / 一个样本都没收到）。
    pub fn has_error(&self) -> bool {
        self.shared.failed.load(Ordering::SeqCst)
    }

    /// 错误描述；无错误时为空串。
    pub fn error(&self) -> String {
        self.shared.error.lock().unwrap().clone()
    }
}

impl Drop for Mp3Writer {
    fn drop(&mut self) {
        self.close();
    }
}

/// 写线程主体：先初始化编码器并回传结果，再循环消费队列，最后 Finalize。
fn writer_thread(
    shared: Arc<Shared>,
    path: Vec<u16>,
    rate: u32,
    channels: u16,
    bitrate_bps: u32,
    init: mpsc::SyncSender<Result<(), String>>,
) {
    // 本线程负责全部 Media Foundation 调用，因此先按 MTA 初始化 COM。
    // 对应 C++ 版：那边是在已 CoInitializeEx 过的 UI 线程上做同样的事。
    // Safety: 在本线程内初始化/反初始化，配对由 RAII 保证。
    let _com = unsafe { ComApartment::mta() };

    let mut sink = match Sink::open(&path, rate, channels, bitrate_bps) {
        Ok(sink) => {
            let _ = init.send(Ok(()));
            sink
        }
        Err(e) => {
            let _ = init.send(Err(e));
            return;
        }
    };
    loop {
        let chunk = {
            let mut queue = shared.queue.lock().unwrap();
            while queue.chunks.is_empty() && queue.running {
                queue = shared.cv.wait(queue).unwrap();
            }
            match queue.chunks.pop_front() {
                Some(c) => c,
                // 队列已排空且 running == false
                None => break,
            }
        };

        if shared.failed.load(Ordering::SeqCst) {
            continue;
        }
        if let Err(e) = sink.write(&chunk) {
            shared.failed.store(true, Ordering::SeqCst);
            *shared.error.lock().unwrap() = e;
        }
    }

    // 与 C++ 版一致：`time == 0` 说明一个样本都没处理过（空流），
    // Finalize 必然失败，直接把空文件删掉。`time` 需要先取出，
    // 因为下面 drop(sink) 之后就访问不到了。
    let written_time = sink.time;

    if !shared.failed.load(Ordering::SeqCst) && written_time == 0 {
        shared.failed.store(true, Ordering::SeqCst);
        *shared.error.lock().unwrap() =
            "未捕获到任何音频数据（该设备可能没有在播放声音），空文件已删除".to_string();
    }

    if !shared.failed.load(Ordering::SeqCst) {
        if let Err(e) = sink.finalize() {
            shared.failed.store(true, Ordering::SeqCst);
            *shared.error.lock().unwrap() = e;
        }
    }

    // 先释放 sink writer，再 MFShutdown（由 Sink 内部的 MfGuard 负责）
    drop(sink);

    if shared.failed.load(Ordering::SeqCst) && written_time == 0 {
        // Safety: path 是本线程持有的合法 NUL 结尾字符串。
        unsafe { DeleteFileW(path.as_ptr()) };
    }
}

/// `MFShutdown` 的 RAII 守卫：保证它与 `MFStartup` 严格配对。
struct MfGuard;

impl Drop for MfGuard {
    fn drop(&mut self) {
        // Safety: 与 Sink::open 中成功的 MFStartup 配对。
        unsafe { MFShutdown() };
    }
}

/// 一路 MP3 Sink Writer。字段顺序刻意让 `writer` 先于 `_mf` 释放。
struct Sink {
    writer: ComPtr<IMFSinkWriter>,
    stream: DWORD,
    rate: u32,
    channels: u16,
    /// 已写入时长，单位 100ns
    time: i64,
    _mf: MfGuard,
}

impl Sink {
    fn open(path: &[u16], rate: u32, channels: u16, bitrate_bps: u32) -> Result<Self, String> {
        // Safety: 每个 Media Foundation 调用都在本线程内完成，参数均为合法值。
        unsafe {
            let hr = MFStartup(MF_VERSION, MFSTARTUP_FULL);
            if failed(hr) {
                return Err(hr_msg("MFStartup 失败", hr));
            }
            match create_sink(path, rate, channels, bitrate_bps) {
                Ok((writer, stream)) => Ok(Self {
                    writer,
                    stream,
                    rate,
                    channels,
                    time: 0,
                    _mf: MfGuard,
                }),
                Err(e) => {
                    // 失败路径：先释放这里创建的对象，再 MFShutdown
                    MFShutdown();
                    Err(e)
                }
            }
        }
    }

    /// 写入一块交错 PCM16。
    fn write(&mut self, samples: &[i16]) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        // Safety: 所有 MF 对象都由本线程创建与使用。
        unsafe {
            let bytes = core::mem::size_of_val(samples) as DWORD;
            let mut buffer = ComPtr::<IMFMediaBuffer>::null();
            let hr = MFCreateMemoryBuffer(bytes, buffer.as_out_ptr());
            if failed(hr) {
                return Err(hr_msg("MFCreateMemoryBuffer 失败", hr));
            }

            let mut dst: *mut u8 = null_mut();
            let hr = buffer.expect_ref().lock(&mut dst, null_mut(), null_mut());
            if failed(hr) {
                return Err(hr_msg("IMFMediaBuffer::Lock 失败", hr));
            }
            core::ptr::copy_nonoverlapping(samples.as_ptr() as *const u8, dst, bytes as usize);
            buffer.expect_ref().unlock();
            buffer.expect_ref().set_current_length(bytes);

            let mut sample = ComPtr::<IMFSample>::null();
            let hr = MFCreateSample(sample.as_out_ptr());
            if failed(hr) {
                return Err(hr_msg("MFCreateSample 失败", hr));
            }
            let hr = sample.expect_ref().add_buffer(buffer.as_ptr());
            if failed(hr) {
                return Err(hr_msg("IMFSample::AddBuffer 失败", hr));
            }

            let frames = samples.len() as i64 / self.channels as i64;
            let duration = frames * 10_000_000 / self.rate as i64;
            sample.expect_ref().set_sample_time(self.time);
            sample.expect_ref().set_sample_duration(duration);
            self.time += duration;

            let hr = self.writer.expect_ref().write_sample(self.stream, sample.as_ptr());
            if failed(hr) {
                return Err(hr_msg("MP3 写入失败", hr));
            }
            Ok(())
        }
    }

    fn finalize(&mut self) -> Result<(), String> {
        // Safety: sink writer 由本线程创建，仍在有效期。
        unsafe {
            let hr = self.writer.expect_ref().finalize();
            if failed(hr) {
                Err(hr_msg("Finalize 失败", hr))
            } else {
                Ok(())
            }
        }
    }
}

/// 创建 sink writer 并完成格式协商。
///
/// # Safety
/// 调用前必须已 `MFStartup`。
unsafe fn create_sink(
    path: &[u16],
    rate: u32,
    channels: u16,
    bitrate_bps: u32,
) -> Result<(ComPtr<IMFSinkWriter>, DWORD), String> {
    let mut writer = ComPtr::<IMFSinkWriter>::null();
    let hr = MFCreateSinkWriterFromURL(path.as_ptr(), null_mut(), null_mut(), writer.as_out_ptr());
    if failed(hr) {
        return Err(hr_msg("创建输出文件失败", hr));
    }

    // ---- 输出类型：MP3 ----
    let mut out_type = ComPtr::<IMFMediaType>::null();
    let hr = MFCreateMediaType(out_type.as_out_ptr());
    if failed(hr) {
        return Err(hr_msg("MFCreateMediaType(输出) 失败", hr));
    }
    {
        let t = out_type.expect_ref();
        t.set_guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
        t.set_guid(&MF_MT_SUBTYPE, &MFAudioFormat_MP3);
        t.set_uint32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
        t.set_uint32(&MF_MT_AUDIO_NUM_CHANNELS, channels as u32);
        t.set_uint32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate_bps / 8);
        t.set_uint32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 1);
        // 注意：不要设置 MF_MT_AUDIO_BITS_PER_SAMPLE 等额外属性，
        // 内置 MP3 编码器枚举的输出类型没有该属性，多了会导致匹配失败
    }
    let mut stream: DWORD = 0;
    let hr = writer.expect_ref().add_stream(out_type.as_ptr(), &mut stream);
    if failed(hr) {
        return Err(hr_msg("AddStream(MP3) 失败，系统可能缺少内置 MP3 编码器", hr));
    }

    // ---- 输入类型：PCM16 ----
    let mut in_type = ComPtr::<IMFMediaType>::null();
    let hr = MFCreateMediaType(in_type.as_out_ptr());
    if failed(hr) {
        return Err(hr_msg("MFCreateMediaType(输入) 失败", hr));
    }
    {
        let t = in_type.expect_ref();
        t.set_guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
        t.set_guid(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
        t.set_uint32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
        t.set_uint32(&MF_MT_AUDIO_NUM_CHANNELS, channels as u32);
        t.set_uint32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        t.set_uint32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels as u32 * 2);
        t.set_uint32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * channels as u32 * 2);
        t.set_uint32(&MF_MT_AUDIO_VALID_BITS_PER_SAMPLE, 16);
        t.set_uint32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1);
    }
    let hr = writer.expect_ref().set_input_media_type(stream, in_type.as_ptr(), null_mut());
    if failed(hr) {
        return Err(hr_msg("设置编码器输入格式失败", hr));
    }

    let hr = writer.expect_ref().begin_writing();
    if failed(hr) {
        return Err(hr_msg("BeginWriting 失败", hr));
    }

    Ok((writer, stream))
}
