# AI 会话索引

| 日期 | 主题 | 结论 | 涉及文件 |
|------|------|------|----------|
| 2026-11-06 | v1.4 收尾 → v1.5：自动分割 + 自动录制失败自愈 | 分割用「编码器槽位」原子替换实现无间隙分段（`..._1/_2`，默认不勾选、60 分钟）；自动录制按会议正在使用的设备启动、失败退避重试 4 次、掉线自愈继续录、麦克风失败降级；`StopReason` 区分手动停止与异常中断；版本升到 1.5/1.5.0；另记录 AI 转写/摘要方案 | `src/{main,recorder,localization,logger,teams_watcher}.{h,cpp}`, `lang/*.ini`, `src_rs/src/{app,teams_watcher}.rs`, `src_rs/src/ffi/system.rs`, `CMakeLists.txt`, `src_rs/Cargo.toml` |
