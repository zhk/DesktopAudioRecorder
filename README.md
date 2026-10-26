# 桌面音频录制 (Desktop Audio Recorder)

Windows 桌面音频录制工具，WASAPI Loopback + 麦克风混音，MP3 编码存储，支持 Teams/微信/钉钉/腾讯会议自动录制。

## 版本
v1.3

## 功能

- 录制系统播放设备输出（桌面音频）+ 可选麦克风
- 实时混音成 MP3（192kbps CBR），保存到程序所在目录
- 可选回声消除（AEC）：消除扬声器声音泄漏到麦克风的回声
- **会议自动录制**：检测 Teams、微信、钉钉、腾讯会议的活跃音频会话，自动开始/停止录制
- 录制文件按 App 分目录存放（Teams/WeChat/DingTalk/TencentMeeting/）
- 系统托盘运行、最小化到托盘
- 多语言界面（简体中文 / English），支持实时切换
- 录制日志（UTF-8）

## 系统要求

- Windows 8 及以上（需内置 MP3 编码器 MFT）
- 不需要额外安装任何运行库，exe 可直接运行

## 技术实现

### 音频采集 (WASAPI Loopback + Microphone)

**技术：Windows Audio Session API (WASAPI)**

- **桌面音频 (Loopback)**：通过 `AUDCLNT_STREAMFLAGS_LOOPBACK` 标志初始化 `IAudioClient`，直接从系统默认播放设备输出流中抓取 PCM 数据，捕获"扬声器正在播放的声音"。采用事件驱动模式 (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`)，100ms 缓冲区。

- **麦克风输入 (Capture)**：另一条独立的 `IAudioClient` 流，采集默认录音设备输入。

- **格式适配**：自动识别 `WAVEFORMATEX` 中的 float32/int16 格式，实时转为 PCM16。

- **多声道降混**：5.1/7.1 声道降混为立体声——前置 L/R 直通 + 0.707×中置 + 环绕均摊混入两侧，丢弃 LFE 低频通道。

- **线程模型**：每个采集流运行在独立 worker 线程中（`avrt.dll` 优先级提升）。

### 回声消除 (AEC)

**算法：NLMS 自适应滤波器 + 双讲检测 (DTD)**

- **参考信号**：桌面 loopback 流作为已知参考，学习"扬声器→房间→麦克风"的泄漏路径
- **滤波器窗口**：4800 taps（100ms @ 48kHz），收敛后形成房间脉冲响应（RIR）
- **归一化步长**：μ = 0.1（能量归一化，避免参考信号音量突变导致发散）
- **双讲检测**：Geigel 功率比阈值 0.25，超过则冻结系数更新 150ms（防抖）
- **能量门控**：安静时直通不处理（`m_norm <= m_eps*1.01`），避免分母趋零

详见 [`src/echo_canceller.{h,cpp}`](src/echo_canceller.h) 与 [`src/audio_mixer.cpp`](src/audio_mixer.cpp)。

### 混音方案 (AudioMixer)

- **桌面流为主时钟**：桌面 loopback 流的采样率/声道数作为输出基准。

- **麦克风重采样对齐**：麦克风设备采样率可能与桌面不同（如 48000Hz vs 44100Hz），通过线性插值实现子帧级精度重采样。

- **背压保护**：麦克风缓冲区设容量上限，超出时丢弃最旧数据；欠载时补静音。

- **混音**：桌面样本和重采样后麦克风样本逐帧相加，软限幅防溢出。


### MP3 编码与存储

**技术：Windows Media Foundation (MF) Sink Writer + 内置 MP3 编码器**

- `MFCreateSinkWriterFromURL` 创建文件写入器，输出 MP3、输入 PCM16，由系统内置 codec 自动编码
- **码率**：192 kbps CBR
- **线程解耦**：采集线程通过 `std::deque` 队列将数据交付给独立编码线程，互不阻塞
- **背压保护**：队列超过约 5 秒数据时丢弃新包
- **文件命名**：`[App名]-YYYYMMDD-HHMMSS.mp3`

### 会议检测

通过 [`IAudioSessionManager2`](src/wasapi_capture.h) 枚举默认播放设备上的活跃音频会话，匹配进程名：

- Teams：`teams.exe` / `ms-teams.exe`
- 微信：`wechat.exe` / `weixin.exe`
- 钉钉：`dingtalk.exe` / `dingtalklite.exe` / `DingTalk.exe`
- 腾讯会议：`wemeet.exe` / `wemeetapp.exe` / `wemeetlauncher.exe`

每 2 秒轮询一次，连续 5 次（10 秒）未检测到会议才自动停止录制（防抖）。

勾选「Teams 会议自动录制」后，检测到会议有活跃音频会话就开始录，会议结束自动停止；手动停止后的同一场会议不再自动重开。

### 整体数据流

```
桌面 Loopback ─┐
               ├─→ AudioMixer ─→ Mp3Writer (Push 入队) ─→ MF Sink Writer ─→ .mp3 文件
麦克风 ────重采样──┘         ↑                ↑
               └─ 可选 AEC ──┘                独立编码线程
```

所有采集和编码组件均不依赖第三方库，纯 Windows 系统 API。

## 编译 (C++ / CMake)

**工具链：CMake 4.4.0 + Visual Studio 18 Professional(2026) MSVC 14.51 + Windows SDK 10.0.26100.0**

```bash
# 方式一：标准 CMake 构建（沙箱环境需改用 build_release.bat）
mkdir build && cd build
cmake .. -G "Visual Studio 17 2022" -A x64
cmake --build . --config Release

# 方式二：一键构建脚本（推荐，绕过 vcvars 的沙箱限制）
cmd //c build_release.bat
```

产物：`build/desktop-audio-recorder.exe`（约 316 KB，x64），`/MT` 静态链接 CRT，无 VC++ Redist 依赖。

**注意**：沙箱环境下 `vcvars64.bat` 会因拦截 `reg.exe` 而失败，此时请直接使用 `build_release.bat`（显式设置 PATH/INCLUDE/LIB）。

## 作者

Zhang Kun (zhk.tiger@gmail.com)

## 法律声明

本软件仅供合法用途。未经他人明确同意，私自录制通话内容在部分地区可能构成违法。使用前请确保遵守当地法律法规。开发者对因使用本软件产生的任何法律责任概不负责。