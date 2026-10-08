//! 双路混音器：桌面(loopback)为主时钟基准，麦克风经线性插值重采样对齐、
//! 可选回声消除后逐样本相加。两路 `push` 可来自不同线程；输出格式 = 桌面格式。
//! 设备时钟漂移的处理：麦克风缓冲设上限（超出丢最旧数据），欠载时补静音。
//!
//! 逻辑与 C++ 版 `src/audio_mixer.{h,cpp}` 一一对应。

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::echo_canceller::EchoCanceller;

/// 混音结果回调（在调用 `push_desktop` 的那个采集线程上触发）。
pub type MixedFn = Box<dyn FnMut(&[i16]) + Send + 'static>;

struct MixerState {
    rate: u32,
    channels: u16,
    on_mixed: Option<MixedFn>,

    mic_enabled: bool,
    mic_rate: u32,
    mic_channels: u16,
    /// 每输出帧消耗的麦克风输入帧数
    mic_step: f64,
    /// `mic_in` 内的小数帧位置
    mic_pos: f64,

    aec_on: bool,
    aec: EchoCanceller,

    /// 麦克风输入缓冲（输入速率，麦克风原生声道数）
    mic_in: VecDeque<i16>,
    /// 麦克风处理后的缓冲（输出速率、输出声道数）
    mic_pcm: VecDeque<i16>,
    /// 重采样临时缓冲
    rs_buf: Vec<i16>,
    mix_buf: Vec<i16>,
}

/// 双路混音器。内部用互斥锁保护状态，因此所有方法都取 `&self`，
/// 可以放进 `Arc` 后同时被两个采集线程使用。
pub struct AudioMixer {
    inner: Mutex<MixerState>,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MixerState {
                rate: 0,
                channels: 0,
                on_mixed: None,
                mic_enabled: false,
                mic_rate: 0,
                mic_channels: 0,
                mic_step: 1.0,
                mic_pos: 0.0,
                aec_on: false,
                aec: EchoCanceller::new(),
                mic_in: VecDeque::new(),
                mic_pcm: VecDeque::new(),
                rs_buf: Vec::new(),
                mix_buf: Vec::new(),
            }),
        }
    }

    /// 输出格式 = 桌面流的采样率/声道数。
    pub fn open(&self, rate: u32, channels: u16, on_mixed: MixedFn) {
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        st.rate = rate;
        st.channels = channels;
        st.on_mixed = Some(on_mixed);
        st.mic_enabled = false;
        st.mic_in.clear();
        st.mic_pcm.clear();
        st.mic_pos = 0.0;
        st.aec_on = false;
    }

    /// 设置麦克风格式；`rate = 0` 表示不录麦克风（桌面流直通）。
    pub fn set_mic_format(&self, rate: u32, channels: u16) {
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        st.mic_rate = rate;
        st.mic_channels = channels;
        st.mic_enabled = rate != 0;
        st.mic_step = if st.mic_enabled { rate as f64 / st.rate as f64 } else { 1.0 };
        if st.mic_enabled && st.aec_on {
            let out_rate = st.rate;
            let mic_ch = st.mic_channels;
            st.aec.configure(out_rate, mic_ch);
        }
    }

    /// 麦克风回声消除（以桌面流为参考）；默认关闭。
    pub fn set_echo_cancellation(&self, on: bool) {
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        st.aec_on = on;
        if st.mic_enabled {
            let out_rate = st.rate;
            let mic_ch = st.mic_channels;
            st.aec.configure(out_rate, mic_ch);
        }
    }

    /// 桌面（loopback）数据入口。`data` 为交错 PCM16，声道数 = `open` 时的声道数。
    pub fn push_desktop(&self, data: &[i16]) {
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        if st.on_mixed.is_none() {
            return;
        }
        let channels = st.channels as usize;
        let frames = data.len() / channels;

        if !st.mic_enabled {
            // 不开麦克风时直通，零开销
            if let Some(cb) = st.on_mixed.as_mut() {
                cb(data);
            }
            return;
        }

        // 桌面流同时作为回声消除的参考信号
        if st.aec_on {
            let ref_ch = st.channels;
            st.aec.push_reference(data, frames as u32, ref_ch);
        }

        let samples = frames * channels;
        st.mix_buf.clear();
        st.mix_buf.resize(samples, 0);
        for (i, &sample) in data[..samples].iter().enumerate() {
            let mut v = sample as i32;
            if let Some(mic) = st.mic_pcm.pop_front() {
                v += mic as i32;
            }
            // 饱和防削波
            st.mix_buf[i] = v.clamp(-32768, 32767) as i16;
        }
        if let Some(cb) = st.on_mixed.as_mut() {
            cb(&st.mix_buf);
        }
    }

    /// 麦克风数据入口。`data` 为交错 PCM16，声道数 = `set_mic_format` 时的声道数。
    pub fn push_mic(&self, data: &[i16]) {
        if data.is_empty() {
            return;
        }
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        if !st.mic_enabled || st.on_mixed.is_none() {
            return;
        }
        let mch = st.mic_channels as usize;

        // 1) 追加原始输入（麦克风原生声道数，输入速率域）
        st.mic_in.extend(data.iter().copied());

        // 2) 重采样到输出速率（线性插值，会议语音足够），保持原生声道数。
        //    统一经 rs_buf 中转：AEC 需要就地修改，而 deque 非连续存储。
        if st.mic_rate == st.rate {
            st.rs_buf.clear();
            st.rs_buf.extend(st.mic_in.drain(..));
            st.mic_pos = 0.0;
        } else {
            st.rs_buf.clear();
            let avail = st.mic_in.len() / mch;
            while st.mic_pos + 1.0 < avail as f64 {
                let i0 = st.mic_pos as usize;
                let frac = st.mic_pos - i0 as f64;
                for ch in 0..mch {
                    let a = st.mic_in[i0 * mch + ch] as i32;
                    let b = st.mic_in[(i0 + 1) * mch + ch] as i32;
                    st.rs_buf.push((a as f64 + (b - a) as f64 * frac) as i16);
                }
                st.mic_pos += st.mic_step;
            }
            // 丢弃已完全消耗的输入帧，保留 floor(mic_pos) 帧供下次插值
            let drop_frames = st.mic_pos as usize;
            for _ in 0..drop_frames * mch {
                if st.mic_in.pop_front().is_none() {
                    break;
                }
            }
            st.mic_pos -= drop_frames as f64;
        }
        let rs_frames = st.rs_buf.len() / mch;

        // 3) 回声消除（以桌面播放流为参考）
        if st.aec_on && rs_frames > 0 {
            st.aec.process(&mut st.rs_buf);
        }

        // 4) 声道映射为输出声道数，进入待混音缓冲
        let out_ch = st.channels as usize;
        for f in 0..rs_frames {
            if mch == out_ch {
                for ch in 0..out_ch {
                    let v = st.rs_buf[f * mch + ch];
                    st.mic_pcm.push_back(v);
                }
            } else if out_ch == 2 && mch == 1 {
                // 单声道麦克风 -> 双声道
                let v = st.rs_buf[f];
                st.mic_pcm.push_back(v);
                st.mic_pcm.push_back(v);
            } else if out_ch == 1 && mch == 2 {
                let l = st.rs_buf[f * 2] as i32;
                let r = st.rs_buf[f * 2 + 1] as i32;
                st.mic_pcm.push_back(((l + r) / 2) as i16);
            }
        }

        // 5) 时钟漂移兜底：缓冲超过约 0.5s 丢弃最旧数据
        let cap = (st.rate / 2) as usize * out_ch;
        while st.mic_pcm.len() > cap {
            st.mic_pcm.pop_front();
        }
    }

    /// 关闭：丢弃回调与所有缓冲。
    pub fn close(&self) {
        let mut guard = self.inner.lock().unwrap();
        let st = &mut *guard;
        st.on_mixed = None;
        st.mic_in.clear();
        st.mic_pcm.clear();
    }
}
