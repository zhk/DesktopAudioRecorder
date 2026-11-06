# 进度时间线

## 已完成

### v1.4 (2026-10)
- Teams 会议检测改为枚举所有活跃播放设备（原只查默认设备，会错过蓝牙/USB 等非默认设备）
- Rust 版会议检测同步为枚举所有活跃设备 + 多 App 支持
- Rust 版 Cargo.toml 版本号更新为 1.4.0（CMakeLists 1.4.0，语言文件 v1.4）
- 启动时写日志：版本号 + 当前选项（语言 / Teams 监控 / 开机启动 / 自动分割 / 分割时长）
- 新增「自动分割录音文件」：复选框（默认不勾选）+ 15/30/60/90 分钟下拉框，默认不勾选，时长默认 60 分钟
  - 分段命名 `xxx-YYYYMMDD-HHMMSS.mp3`、`..._1.mp3`、`..._2.mp3`（首段无后缀）
  - 切换分段时不重启采集线程：先建新编码器，加锁原子替换，再关闭旧编码器
  - 配置持久化 `[Options] auto_split=`, `split_minutes=`（复选框和下拉框任一改变立即写 ini）
  - 新分段打开失败时：本次录制停止分割，已录内容不中断
  - 新语言 key：`[UI] chk_auto_split`(=自动分割录音文件 / Auto Split Recording), `split_15`, `split_30`, `split_60`, `split_90`
- 控件换位：第 3 行（y=118）= 自动分割录音文件 + 分割时长下拉框；第 4 行（y=146）= AEC + Teams 自动录音
- 失败事件全部进日志：开始录制失败（无设备 / 采集失败 / 麦克风失败 / 编码器失败）、采集线程运行期错误（`WM_RECORD_ERROR`，之前只弹窗不写日志）、分割打开失败；多行错误压成单行（C++ `AppendLogLine`，Rust `one_line`）
- Rust 补齐 C++ 已有行为：录音按会议 App 子目录/前缀命名（`make_output_base`）；采集错误按「中断」记账（`pending_error`）

### v1.3 (2026-10)
- 音频设备变更自动检测刷新 (`device_notify.h`)
- 开机自动启动 checkbox + 注册表写入
- Checkbox 状态持久化到配置文件 (`[Options] auto_teams=`, `auto_start=`)
- 单进程限制 (`FindWindowW`)

### v1.2 (2026-10)
- 页面重新布局：开始录制/已录/打开文件夹同行，AEC/Teams checkbox 同行
- Teams checkbox 加宽到 260px
- 状态框高度增加 20px
- 语言菜单 ID 掩码修复
- EnumChildWindows 跳过控件修复
- ini 加载从 `GetPrivateProfileSectionW` 改为直接读取 UTF-16LE

### v1.1 (2026-10)
- 钉钉会议支持 (`dingtalk.exe` / `DingTalk.exe`)
- 录音文件按 App 前缀命名 (`Teams-`, `WeChat-`, `DingTalk-`)
- 各 App 录音存入独立子目录
- 腾讯会议支持 (`wemeet.exe` / `wemeetapp.exe`)

### v1.0 (2026-10)
- 基础的 WASAPI Loopback + Mic 录制
- MP3 编码（192 kbps, Media Foundation）
- 回声消除（NLMS + DTD）
- Teams 和微信自动录制
- 多语言界面（中/英 + 系统语言检测）
- 系统菜单（关于、语言切换）
- 录制日志（UTF-8, 按月切割）
- 系统托盘
- 双版本（C++ + Rust）
- 错误信息多语言 (`[Errors]` 节)
- 代码文件拆分 (`localization/logger/recorder` 从 main.cpp 分离)
- Allman 代码风格统一

## 待办

### 高优先级
- [ ] C++ 版录制中状态框不刷新：`TIMER_ID` 从未 `SetTimer`（Rust 版是 500ms），只有开始录制时写一次
- [ ] Rust 版语言切换无效：`LOCALE` / `STRINGS` 是 `OnceLock`，第二次 `set()` 静默失败

### 中优先级
- [ ] 日志旧文件自动清理
- [ ] 添加退出确认
- [ ] 安装程序

### 低优先级 / 搁置
- [ ] 更多语言
- [ ] 进度条
- [ ] 视频录制（不在范围内）
- [ ] Web UI / 远程控制