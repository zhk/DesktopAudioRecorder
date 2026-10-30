# 进度时间线

## 已完成

### v1.4 (2026-10)
- Teams 会议检测改为枚举所有活跃播放设备（原只查默认设备，会错过蓝牙/USB 等非默认设备）
- Rust 版会议检测同步为枚举所有活跃设备 + 多 App 支持
- Rust 版 Cargo.toml 版本号从 1.0.0 更新为 1.3.0

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
- [ ] Rust 版 Cargo.toml 版本号更新

### 中优先级
- [ ] 日志旧文件自动清理
- [ ] 添加退出确认
- [ ] 安装程序

### 低优先级 / 搁置
- [ ] 更多语言
- [ ] 进度条
- [ ] 视频录制（不在范围内）
- [ ] Web UI / 远程控制