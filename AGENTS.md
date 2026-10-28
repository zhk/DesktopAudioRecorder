# AGENTS.md — 与 AI Agent 的工作协议

## 项目背景（30 秒版）

Windows 桌面音频录制 exe：WASAPI Loopback 抓取播放设备输出 + 麦克风混音，Windows 内置 MP3 编码器输出。
双版本：C++17/CMake（主）和 Rust/Cargo（等价实现）。Win32 GUI，系统托盘，多语言。零第三方库。

## 开工前必读顺序

1. 本文件 (AGENTS.md) — 规则
2. README.md — 项目地图
3. docs/architecture.md — 系统架构
4. progress.md — 当前进度
5. memory/pitfalls.md — 已知坑位
6. 如涉及 AEC，读 docs/aec.md

## 常用命令

| 用途 | C++ | Rust |
|------|-----|------|
| 构建 Release | `cd build && cmake --build . --config Release` | `cd src_rs && cargo build --release` |
| 首次配置 | `mkdir build && cd build && cmake .. -G "Visual Studio 17 2022" -A x64` | — |
| 沙箱构建 | `build_release.bat` | — |
| 同步 ini | `Copy-Item lang\*.ini build\Release\lang\ -Force` | — |
| 图标生成 | `pwsh make-icon.ps1` | 同 C++ |

## 目录与数据存放位置

| 类型 | 位置 | 说明 |
|------|------|------|
| 源码 | `src/` (C++), `src_rs/src/` (Rust) | 核心代码 |
| 文档 | `docs/` | 技术文档 |
| 语言文件 | `lang/` | UTF-16LE ini |
| 会话记录 | `ai-sessions/` | 每次 AI 会话后写入 |
| 进度/坑位 | `memory/` | 当前状态 |
| 构建产物 | `build/`, `src_rs/target/` | 生成目录 |
| 禁止写入 | `build/Release/` (只能构建产出，不能手动修改) | |

## 边界：什么可以自作主张，什么必须问我

### 可以自主进行

- 阅读任何文件
- 编译和构建（Release 和 Debug）
- 创建/修改 `docs/`、`memory/`、`ai-sessions/` 中的文档文件
- 修改 `lang/*.ini` 中的翻译文本（但必须保持一致）
- 修改 Rust 版的 Cargo.toml 元数据
- 搜索代码、分析问题

### 必须先问我

- 修改任何 `src/` 或 `src_rs/src/` 中的源码文件
- 任何涉及删除用户数据或生成文件的改动
- 添加新功能、改变已有行为
- 添加第三方依赖
- 合并 C++/Rust 两版的差异

### 绝对禁止

- 修改 `.git/` 下的任何内容
- 提交代码到 git（除非明确要求）
- 删除或覆盖已有的 `lang/*.ini` 中的已有 key
- 更改构建系统的工具链路径
- 在源码中引入硬编码的中文/英文文本（必须通过 `S()` 或 `tr()` 走语言文件）
- 修改 `device_notify.h` 中的消息 ID（会破坏两个版本的一致性）
- 执行需要管理员权限的操作
- 修改 `build/` 或 `src_rs/target/` 中的生成文件

## 必须满足的标准

- 修改功能后，C++ 和 Rust 两版要保持一致
- 新加的 UI 文本必须同步更新 `lang/zh-CN.ini` 和 `lang/en-US.ini`
- 新加的功能必须同时支持中英文
- 修改后必须编译通过（`cmake --build . --config Release` 和 `cargo build --release`）
- 每次完成需求后更新 `progress.md`
- 每次会话结束后在 `ai-sessions/` 写入记录

## 规则优先级

```
本次会话中的用户明确指令
    >
绝对禁止
    >
docs/architecture.md 中的设计约束
    >
本文件 (AGENTS.md)
    >
既有代码惯例（如代码风格、命名规范）
    >
框架默认
```

## 回答方式

- 不用 emoji
- 简洁准确，先结论后理由
- 不确定要说"不确定"，不要猜测填充
- 需要用户决定的事项给出清晰的选择题
- 编译错误直接展示关键行，不要输出完整日志

## 每次会话收尾必做

1. 如果涉及行为/功能变更，更新 `progress.md`
2. 在 `ai-sessions/` 写入本次会话记录（按 `ai-sessions/INDEX.md` 中的格式）
3. 记录决策与踩过的坑
4. 如果涉及源码修改，确认两个版本（C++/Rust）的同步状态