# 当前状态

## 已完成

### 核心录制
- [x] WASAPI Loopback 桌面音频采集
- [x] WASAPI Capture 麦克风采集
- [x] 双路混音 + 线性插值重采样
- [x] NLMS 回声消除（AEC）+ 双讲检测（DTD）
- [x] Media Foundation MP3 编码（192 kbps CBR）
- [x] 多声道降混（5.1/7.1 → 立体声）
- [x] 独立编码线程（采集/编码解耦）

### GUI
- [x] Win32 窗口（520×306）
- [x] 播放设备/麦克风下拉框
- [x] 设备刷新按钮
- [x] 开始/停止录制按钮
- [x] 打开文件夹按钮
- [x] 已录数量显示
- [x] 状态信息框
- [x] 回声消除 checkbox
- [x] 自动录制 checkbox（Teams/微信/钉钉/腾讯会议）
- [x] 开机启动 checkbox
- [x] 自动分割录音文件 checkbox + 时长下拉框（15/30/60/90 分钟，默认 60）
- [x] 系统托盘 + 最小化到托盘
- [ ] 进度条显示（未实现）

### 多语言
- [x] 简体中文 (`lang/zh-CN.ini`)
- [x] 美国英文 (`lang/en-US.ini`)
- [x] 实时切换语言
- [x] 系统语言自动检测
- [x] 用户选择持久化到配置文件

### 会议检测
- [x] Teams (teams.exe / ms-teams.exe)
- [x] 微信 (wechat.exe / weixin.exe)
- [x] 钉钉 (dingtalk.exe / dingtalklite.exe / DingTalk.exe)
- [x] 腾讯会议 (wemeet.exe / wemeetapp.exe / wemeetlauncher.exe)
- [x] 音频会话枚举 + 进程名匹配
- [x] 识别「会议正在使用的播放设备」（会话 PID 命中端点 → 设备 ID + 友好名）
- [x] 防抖（10s 不活跃才停录）

### 设备管理
- [x] 默认设备自动切换
- [x] 设备变更通知 (`IMMNotificationClient`)
- [x] 设备失效检测 (`AUDCLNT_E_DEVICE_INVALIDATED`)
- [x] 录制中设备掉线自愈：换设备继续录，后续写入新的分割文件
- [x] 刷新设备但保留用户当前选择

### 配置与日志
- [x] `exe名.ini` 用户配置（语言、checkbox 状态、自动分割时长）
- [x] UTF-8 with BOM 日志文件（按月切割）
- [x] 录制开始/结束/错误/设备变更/会议启动记录
- [x] 失败事件写日志（开始失败、运行期中断、分割失败、重试与放弃）
- [x] 开机自动启动注册表写入
- [x] 单进程限制

### 文档
- [x] README.md (项目地图)
- [x] AGENTS.md
- [x] docs/architecture.md
- [x] docs/aec.md
- [x] docs/adr/0001-initial-architecture.md
- [x] memory/pitfalls.md
- [x] progress.md
- [x] ai-sessions/INDEX.md

### C++/Rust 双版本
- [x] C++ 版（`src/`，CMake，MSVC）
- [x] Rust 版（`src_rs/`，Cargo，零 crate 依赖）
- [x] 功能完全等价

## 进行中

- [ ] v1.5 — 真机验证：录制中切换/拔掉播放设备能否自愈、麦克风被占用能否降级、会议中设备消失的 4 次重试

## 待办

- [ ] 日志旧文件自动清理
- [ ] 添加退出确认（防止误关）
- [ ] 平衡电量/性能的设置选项
- [ ] 更多语言支持
- [ ] 安装程序 / 打包脚本

## 明确搁置

- [ ] 视频录制（不在项目范围内）
- [ ] GPU 加速（无需求）
- [ ] Web UI / 远程控制（复杂度过高）