# 桌面音频录制（Rust 版）

Windows 桌面音频录制小工具，功能与仓库根目录下的 C++ 版本完全一致：

- **WASAPI Loopback** 录制播放设备输出的声音（会议里对方的声音）
- 可选同时录制**麦克风**（自己的声音），线性插值重采样对齐 + 可选 NLMS 回声消除
- **Media Foundation** 内置 MP3 编码器，192 kbps，输出到程序所在目录
- 简单的 Win32 界面、最小化到**系统托盘**
- **Teams 会议看门狗**：检测到会议自动开始录制，会议结束 10s 后自动停止

采集逻辑与 C++ 版一样取自 OBS 的 `plugins/win-wasapi` 思路，不依赖 libobs / OBS 源码树。

---

## 为什么没有第三方依赖

目标环境访问不到 crates.io（`cargo fetch` 直接 SSL 失败），所以本项目**刻意不引入任何 crate**
（包括 `windows` / `windows-sys`）。所有 Win32 / COM / Media Foundation 接口都在
[`src/ffi/`](src/ffi) 下按 Windows SDK 头文件（10.0.26100.0）**手写声明**，直接链接系统 DLL：

| DLL | 用途 |
|-----|------|
| `user32` / `gdi32` / `comctl32` / `shell32` | 窗口、控件、字体、托盘图标 |
| `ole32` | COM 初始化、`CoCreateInstance`、`PropVariantClear` |
| `avrt` | 采集线程提升为 "Pro Audio" 优先级 |
| `mfplat` / `mfreadwrite` | Media Foundation MP3 编码 |

接口的 IID、属性键（`MF_MT_*`、`PKEY_Device_FriendlyName`）、格式子类型等 GUID 都是
照着 SDK 头文件抄下来的常量，因此也**不需要** `uuid.lib` / `mfuuid.lib`。

> COM vtable 的方法顺序即 ABI：每个 `*Vtbl` 结构体上方的注释标明了对应的头文件，
> 顺序与该头文件里 `MIDL_INTERFACE` 之后的声明顺序逐字对应，便于日后核对。

---

## 构建与运行

要求：Rust **1.82+**（用到 `unsafe extern`、`f32::round_ties_even`）、MSVC 工具链、
Windows 8 以上。

```powershell
cd src_rs

cargo build --release            # 产物：target\release\desktop-audio-recorder.exe
cargo run                        # 调试跑一遍

cargo run --example smoke        # 冒烟自检（见下）
cargo clippy --all-targets       # 无警告
```

**请在 `src_rs` 目录内构建**：[`.cargo/config.toml`](.cargo/config.toml) 打开了
`+crt-static`（等价于 C++ 版的 `/MT`），而 cargo 只读取「当前目录及其祖先」下的配置。
这样编出来的 exe 不依赖 VC++ Redist，拷到任意 Win8+ 机器即可运行
（实测 release 版约 **379 KB**，二进制里查不到 `VCRUNTIME140.dll` / `ucrtbase.dll`）。

### 图标与主题清单

对应 C++ 版的两件事：

1. `src/app.rc` 的 `IDI_APP ICON "icon.ico"` —— [build.rs](build.rs) 在构建期调用
   Windows SDK 的 `rc.exe` 编译 [app.rc](app.rc) 并把 `.res` 交给链接器；
   **找不到 `rc.exe` 时只警告不报错**，程序照常编译，只是用系统默认图标。
2. `#pragma comment(linker, "/manifestdependency:...Common-Controls...")` ——
   通过链接参数 `/MANIFEST:EMBED` + `/MANIFESTDEPENDENCY` 嵌入，让控件使用系统主题。

---

## 文件对应关系

| C++（`../src`） | Rust（`src/`） | 说明 |
|---|---|---|
| `main.cpp` | [`app.rs`](src/app.rs) + [`main.rs`](src/main.rs) | 窗口/控件/托盘/看门狗 |
| `wasapi_capture.{h,cpp}` | [`wasapi_capture.rs`](src/wasapi_capture.rs) | WASAPI loopback / 麦克风采集 |
| `audio_mixer.{h,cpp}` | [`audio_mixer.rs`](src/audio_mixer.rs) | 双路混音、重采样、声道映射 |
| `echo_canceller.{h,cpp}` | [`echo_canceller.rs`](src/echo_canceller.rs) | NLMS 回声消除 + 双讲检测 |
| `mp3_writer.{h,cpp}` | [`mp3_writer.rs`](src/mp3_writer.rs) | MF Sink Writer 封装 + 写入队列 |
| `teams_watcher.{h,cpp}` | [`teams_watcher.rs`](src/teams_watcher.rs) | 音频会话检测会议状态 |
| 系统头文件 | [`ffi/`](src/ffi) | 手写 FFI：`system.rs` / `wasapi.rs` / `mf.rs` |
| `resource.h` / `app.rc` | [`resource.h`](resource.h) / [`app.rc`](app.rc) | 图标资源 |
| — | [`util.rs`](src/util.rs) | UTF-16 转换 |
| — | [`examples/smoke.rs`](examples/smoke.rs) | 冒烟自检 |

可执行文件入口是 [`main.rs`](src/main.rs)（`#![windows_subsystem = "windows"]`，
对应 C++ 的 `WIN32` 可执行目标），其余逻辑都在库里，便于单独测试。
C++ 的 CMake 构建**未做改动**，两套代码互不影响。

---

## 与 C++ 版有意的实现差异

移植时保持了逐样本的算法等价（浮点字面量、舍入方式、缓冲上限、防抖计数都没动），
但下面几处为了 Rust 的安全性/惯例做了调整：

1. **COM 接口方法取 `&self` 而不是 `self`。**
   `IUnknown*` 本身就是"指向 vtable 指针的指针"，即接口结构体内存的地址就是 `this`。
   如果按值接收 `self`，`self` 会是栈上的一份拷贝，把拷贝地址当 `this` 传进 COM 会让
   `Release` / `CONTAINING_RECORD` 这类依赖对象地址的实现写坏内存。因此所有 vtable
   调用都通过 `&self` 取地址（详见 [`ffi/mod.rs`](src/ffi/mod.rs) 中 `Com` 的文档）。

2. **Media Foundation 全部调用都在写线程里完成。**
   C++ 在主线程创建 sink writer、在写线程里使用；Rust 版让写线程自己
   `MFStartup` → 建 sink → 消费队列 → `Finalize` → `MFShutdown`，并通过 channel
   把初始化结果回传给 `open()`（阻塞语义与 `WasapiCapture::start` 一致）。
   这样 COM 指针始终不出现在 `Send` 边界上，不需要 `unsafe impl Send`。

3. **会泵消息的调用放在借用释放之后。**
   GUI 状态用 `thread_local!` + `RefCell` 表达（窗口过程只在 UI 线程跑），
   但 `MessageBoxW` 与 `TrackPopupMenu` 自己会泵消息，期间嵌套的 `WM_TIMER` 会再次借用
   同一份状态。因此需要弹窗时把内容放进 `PendingMessage` 返回、在借用结束后再显示；
   托盘菜单用 `TPM_RETURNCMD` 直接拿返回值，避免它再发一趟 `WM_COMMAND`。

4. **采集线程的运行期错误**通过 `PostMessageW(WM_RECORD_ERROR, Box<String> 裸指针)`
   投递到 UI 线程，与 C++ 传 `new std::wstring` 的做法一致；窗口句柄用
   `SendableHwnd` 包装（`PostMessage` 本身线程安全，理由写在代码注释里）。

5. **`panic = "abort"` + `strip`**（release），与 C++ 的"静态 CRT、单文件 exe"取向一致。

---

## 冒烟自检

`cargo run --example smoke` 会直接在真实系统上跑一遍关键路径，用来验证手写 FFI 的
vtable 顺序、结构体布局与 GUID 常量：

| 检查项 | 覆盖的接口 |
|---|---|
| 1. 设备枚举 | `IMMDeviceEnumerator` / `IMMDeviceCollection` / `IMMDevice` / `IPropertyStore` / `PROPVARIANT` |
| 2. 音频会话枚举 | `IAudioSessionManager2` / `IAudioSessionEnumerator` / `IAudioSessionControl(2)`（Teams 看门狗路径） |
| 3. MP3 编码 | `IMFSinkWriter` / `IMFMediaType` / `IMFSample` / `IMFMediaBuffer`，并校验输出里的 MP3 帧同步字 |
| 4. 实时采集全链路 | 真实 loopback 采集 → 混音 → MP3，与 GUI 里的接线方式相同 |
| 5. 混音器 / AEC | 直通、逐样本相加、饱和防削波；回声路径收敛后的抑制量 |

第 4 项需要默认播放设备上**确实有声音在播放**才有数据（loopback 的特性）；如果 3 秒内
什么都没抓到，自检会打印提示并按通过处理（此时编码器报告"未捕获到任何音频数据"并删除
空文件，是预期行为）。

本机实测结果（`cargo run --example smoke`，播放一段 441Hz 测试音）：

```
== 设备枚举（MMDevice API）             PASS
== 音频会话枚举（Teams 看门狗路径）      PASS
== MP3 编码（Media Foundation Sink Writer） PASS
== 实时 loopback 采集 -> 混音 -> MP3 全链路
   loopback 格式: 48000 Hz, 2 ch
   3 秒内采集到 144480 帧（约 3.01s 音频），峰值振幅 12000
   采集 -> 混音 -> MP3：71855 字节            PASS
== 混音器 / 回声消除数值健全性
   回声消除：收敛后（最后 0.5s）残差下降 17.2 dB    PASS
```

另外验证过：release exe 能正常创建窗口（标题 `桌面音频录制 (Desktop Audio Recorder)`）
并驻留消息循环。

---

## 使用说明

与 C++ 版完全相同：

1. 「播放设备」选要录的播放设备（默认设备会被默认选中），「麦克风」可选「（不录制）」。
2. 「回声消除 (AEC)」默认勾选：以 loopback 流为参考，从麦克风信号里减掉扬声器回声，
   避免把对方的声音又录一遍（用扬声器外放时才有意义，戴耳机可以关掉省 CPU）。
3. 点「开始录制」，MP3 保存在**程序所在目录**，文件名 `recording-YYYYMMDD-HHMMSS.mp3`。
4. 勾选「Teams 会议自动录制」后，检测到 Teams 有活跃音频会话就开始录，
   会议结束（连续 10s 检测不到）自动停止；手动停止后的同一场会议不再自动重开。
5. 最小化会缩到系统托盘，托盘图标右键可「打开 / 退出」。

注意：MP3 编码用的是系统内置编码器，**Windows N 版需要安装 Media Feature Pack**。
