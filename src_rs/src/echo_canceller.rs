//! 简易 NLMS（归一化最小均方）自适应回声消除，带双讲检测（DTD）。
//! 逻辑与 C++ 版 `src/echo_canceller.{h,cpp}` 一一对应。
//!
//! 原理：桌面播放流是已知的（loopback 参考信号），自适应滤波器学习
//! "扬声器 -> 房间 -> 麦克风" 的泄漏路径，从麦克风信号中减去回声估计。
//! 双讲（本地说话）时语音对自适应算法是干扰，检测到后冻结系数更新，
//! 仅用已收敛的滤波器继续做回声相减。
//!
//! 关键约束：参考流与麦克风流按样本 1:1 对齐消费（两者的相对偏移是常数，
//! 由滤波器窗口吸收）；因此参考信号先进入 FIFO，`process` 时逐样本取出。
//! 两设备时钟漂移通过 FIFO 上限丢旧数据兜底。

use std::collections::VecDeque;

/// 默认回声窗口长度（ms），与 C++ 版的 `tailMs = 100` 一致。
pub const DEFAULT_TAIL_MS: u32 = 100;
/// 默认 NLMS 步长，与 C++ 版的 `step = 0.1f` 一致。
pub const DEFAULT_STEP: f32 = 0.1;

/// 带双讲检测的单通道组 NLMS 回声消除器。
pub struct EchoCanceller {
    rate: u32,
    /// 麦克风声道数（1 或 2）
    ch: u16,
    /// 滤波器长度（taps）
    w: u32,
    mu: f32,

    /// 待消费的参考样本（单声道，输出速率域）
    ref_fifo: VecDeque<f32>,
    /// FIFO 上限（时钟漂移兜底）
    ref_cap: usize,

    /// NLMS 窗口环形缓冲，长度 `w`
    win: Vec<f32>,
    /// 下一个写入位置（== 最旧样本位置）
    pos: u32,
    /// 窗口内参考能量，增量维护
    norm: f64,
    /// 能量下限：低于此值认为无播放声音，直通不处理
    eps: f64,

    /// 每声道一组滤波器系数
    h: Vec<Vec<f32>>,

    // 双讲检测：指数平滑的参考/麦克风短时功率 + 挂起计数
    p_ref: f64,
    p_mic: f64,
    /// 平滑系数（约 50ms 时间常数）
    k: f64,
    /// 双讲挂起剩余样本数
    hold: u32,
    /// 挂起时长（约 150ms）
    hold_max: u32,
    /// Geigel 功率比阈值（幅度比 0.5）
    dtd_ratio: f64,
}

impl Default for EchoCanceller {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoCanceller {
    pub fn new() -> Self {
        Self {
            rate: 0,
            ch: 0,
            w: 0,
            mu: 0.25,
            ref_fifo: VecDeque::new(),
            ref_cap: 0,
            win: Vec::new(),
            pos: 0,
            norm: 1e-6,
            eps: 1.0,
            h: Vec::new(),
            p_ref: 0.0,
            p_mic: 0.0,
            k: 0.0,
            hold: 0,
            hold_max: 0,
            dtd_ratio: 0.25,
        }
    }

    /// 用默认参数配置（对应 C++ 的 `Configure(rate, channels)`）。
    pub fn configure(&mut self, sample_rate: u32, channels: u16) {
        self.configure_ex(sample_rate, channels, DEFAULT_TAIL_MS, DEFAULT_STEP);
    }

    /// `channels` 为麦克风声道数（1 或 2），`tail_ms` 为回声窗口长度。
    pub fn configure_ex(&mut self, sample_rate: u32, channels: u16, tail_ms: u32, step: f32) {
        self.rate = sample_rate;
        self.ch = if channels == 0 { 1 } else { channels };
        self.w = sample_rate * tail_ms / 1000;
        self.mu = step;

        self.ref_fifo.clear();
        // 250ms：参考流快于麦克风时丢弃最旧数据
        self.ref_cap = (sample_rate / 4) as usize;

        self.win = vec![0.0f32; self.w as usize];
        self.pos = 0;
        self.eps = self.w as f64 * 100.0;
        self.norm = self.eps;

        self.h = vec![vec![0.0f32; self.w as usize]; self.ch as usize];

        self.p_ref = 0.0;
        self.p_mic = 0.0;
        // ~50ms 时间常数
        self.k = 1.0 - (-1.0 / (sample_rate as f64 * 0.05)).exp();
        self.hold = 0;
        // 150ms 挂起
        self.hold_max = sample_rate / 1000 * 150;
    }

    /// 清空历史状态（滤波器系数、参考 FIFO、双讲状态）。
    pub fn reset(&mut self) {
        self.ref_fifo.clear();
        self.win.iter_mut().for_each(|v| *v = 0.0);
        self.pos = 0;
        self.norm = self.eps;
        for h in &mut self.h {
            h.iter_mut().for_each(|v| *v = 0.0);
        }
        self.p_ref = 0.0;
        self.p_mic = 0.0;
        self.hold = 0;
    }

    /// 滤波器长度（taps）。
    pub fn tail(&self) -> u32 {
        self.w
    }

    /// 追加桌面播放流到参考 FIFO（任意声道数，内部混缩为单声道）。
    pub fn push_reference(&mut self, data: &[i16], frames: u32, ref_channels: u16) {
        if self.w == 0 || ref_channels == 0 {
            return;
        }

        for f in 0..frames as usize {
            let x = if ref_channels == 1 {
                data[f] as f32
            } else {
                let base = f * ref_channels as usize;
                let l = data[base] as i32;
                let r = data[base + 1] as i32;
                (l + r) as f32 * 0.5
            };
            self.ref_fifo.push_back(x);
        }
        while self.ref_fifo.len() > self.ref_cap {
            self.ref_fifo.pop_front();
        }
    }

    /// 就地处理麦克风流（声道数 = `configure` 的 `channels`），逐样本消费参考 FIFO。
    // 环形缓冲的遍历刻意用"递增索引 + 回绕"而不是取模/迭代器：既与 C++ 版逐字对应，
    // 也避免在每样本热路径里做除法取模。
    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, mic: &mut [i16]) {
        if self.w == 0 || self.ch == 0 {
            return;
        }
        let ch = self.ch as usize;
        let frames = mic.len() / ch;

        for f in 0..frames {
            // 逐样本取参考，保证参考/麦克风 1:1 对齐消费
            let x = self.ref_fifo.pop_front().unwrap_or(0.0);

            let old = self.win[self.pos as usize];
            self.norm += x as f64 * x as f64 - old as f64 * old as f64;
            if self.norm < self.eps {
                self.norm = self.eps;
            }
            self.win[self.pos as usize] = x;
            self.pos += 1;
            if self.pos == self.w {
                self.pos = 0;
            }

            // 没有播放声音时不存在回声，直通（也省 CPU）
            if self.norm <= self.eps * 1.01 {
                continue;
            }

            // 双讲检测（Geigel 功率比 + 挂起）：麦克风功率显著高于参考功率
            // 说明本地在说话（回声是参考的衰减泄漏，正常不会超过该比例）
            {
                let s0 = mic[f * ch] as f64;
                self.p_mic += (s0 * s0 - self.p_mic) * self.k;
            }
            self.p_ref += (x as f64 * x as f64 - self.p_ref) * self.k;
            if self.p_mic > self.dtd_ratio * self.p_ref {
                self.hold = self.hold_max;
            } else if self.hold > 0 {
                self.hold -= 1;
            }
            let freeze = self.hold > 0;

            // 拆分字段借用：win 只读、h 只写，二者互不重叠
            let win = &self.win;
            let w = self.w;
            let pos = self.pos;
            let norm = self.norm;
            let mu = self.mu;

            for c in 0..ch {
                let si = f * ch + c;
                let s = mic[si] as f32;
                let h = &mut self.h[c];

                // 回声估计：y = h · x；环形缓冲满后 pos 处即最旧样本，
                // h[i] 对应窗口内第 i 旧（i=0 最旧，i=w-1 最新）的参考样本
                let mut y = 0.0f32;
                let mut ri = pos;
                for i in 0..w as usize {
                    y += h[i] * win[ri as usize];
                    ri += 1;
                    if ri == w {
                        ri = 0;
                    }
                }

                let e = s - y; // 残差 = 去回声后的信号

                // NLMS 系数更新（双讲时冻结，避免语音干扰滤波器收敛）
                if !freeze {
                    let g = (mu as f64 * e as f64 / (norm + 1e-9)) as f32;
                    ri = pos;
                    for i in 0..w as usize {
                        h[i] += g * win[ri as usize];
                        ri += 1;
                        if ri == w {
                            ri = 0;
                        }
                    }
                }

                // 与 C 的 lrintf 一致：按"就近取偶"舍入后再饱和到 i16
                let v = e.round_ties_even();
                mic[si] = if v > 32767.0 {
                    32767
                } else if v < -32768.0 {
                    -32768
                } else {
                    v as i16
                };
            }
        }
    }
}
