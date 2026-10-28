# ADR 0001: 初始架构决策

## 状态

已接受（2024 年首次实现时确定）

## 背景

需要一款 Windows 桌面音频录制工具，能同时录制系统播放声音和麦克风输入，输出 MP3。

## 决策

### 1. 使用 WASAPI Loopback 而非虚拟音频驱动

**选择**: 直接使用 Windows Audio Session API 的 loopback 模式。

**证据**: `src/wasapi_capture.cpp:InitLoopback()` 使用 `AUDCLNT_STREAMFLAGS_LOOPBACK` 标志。

**理由**: 
- 零安装，不需虚拟音频设备驱动
- 捕获的是最终混音输出（经过系统音量、应用音量后）
- 参考 OBS 的 `plugins/win-wasapi` 插件实现

**代价**: 无法录制"播放设备静音但应用在播放"的场景（逻辑上合理）。

### 2. 使用 Media Foundation 内置 MP3 编码器

**选择**: 通过 `MFCreateSinkWriterFromURL` + `MFAudioFormat_MP3` 编码输出。

**证据**: `src/mp3_writer.cpp:Open()` 设置 output type 为 `MFAudioFormat_MP3`。

**理由**:
- Windows 8+ 内置，无第三方依赖
- 192 kbps CBR 满足语音录制质量需求
- SinkWriter 封装了编码、复用、文件 I/O

**代价**: Windows N/KN 版本需手动安装 Media Feature Pack。

### 3. 双版本（C++ + Rust）

**选择**: 同时维护 C++17 和 Rust 两个等价实现。

**证据**: `src/` 和 `src_rs/` 目录各自独立。

**理由**:
- C++ 版为参考实现
- Rust 版为 Rust 技术栈验证
- 两版共享 ini 配置文件、ico 资源、构建脚本

**代价**: 功能变更需同步两个版本。

### 4. 零第三方库

**选择**: C++ 版只用 Windows SDK + STL；Rust 版手写 FFI。

**证据**: CMakeLists.txt 只链接 `ole32 mfplat mfreadwrite mfuuid avrt comctl32`；`Cargo.toml` 的 `[dependencies]` 为空。

**理由**: 目标机器可能无法访问包管理器或 crates.io。

**代价**: Rust 版需手写 Win32/COM/MF 接口声明（约 2000 行 FFI 代码）。

### 5. 全局变量 + 窗口过程架构（C++）

**选择**: 使用匿名 namespace 全局变量 + 标准 WndProc 回调。

**证据**: `src/main.cpp` 中所有 `g_` 前缀的变量。

**理由**: Win32 SDK 原生模式，无框架依赖，开发直接。

**代价**: 状态管理分散，模块间通过 extern 共享全局变量（`recorder.h` 中声明）。

### 6. 录制文件名按 App 标识前缀

**选择**: 自动检测到的会议 App 名（Teams/WeChat/DingTalk/TencentMeeting）作为文件名前缀，手动录制用 `recording-`。

**证据**: `src/recorder.cpp:MakeOutputPath()` 使用 `g_meetingApp` 变量。

**理由**: 方便区分录音来源。

**代价**: 名字较长（如 `Teams-YYYYMMDD-HHMMSS.mp3`），但对检索友好。

### 7. 日志按月切割

**选择**: 日志文件名含 `YYYYMM`，使用 UTF-8 with BOM 编码。

**证据**: `src/logger.cpp:AppendLog()` 生成 `[exe名]-YYYYMM.log`。

**理由**: 避免单文件过大，方便追溯。

**代价**: 日志搜索需要知道月份。未实现自动清理旧日志。

## 替代方案（已知但未采用）

| 方案 | 为什么不选 |
|------|------------|
| audioworklets / Windows Audio Stream API | 更低延迟但 API 更复杂，当前场景不需要 |
| LAME 编码器 | 需额外 DLL，违反零依赖原则 |
| Vulkan/GPU 加速编码 | 音频编码场景不适用，增加复杂度 |
| Electron / React 桌面框架 | 体积大，依赖多，与轻量目标不符 |
| 注册为 Windows 服务 | 交互式 GUI 场景不需要常驻后台 |
| Plugin 架构（支持自定义编码器） | 当前需求单一，过度设计 |