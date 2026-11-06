# AI 会话索引

| 日期 | 主题 | 结论 | 涉及文件 |
|------|------|------|----------|
| 2026-11-06 | v1.4 收尾 + 自动分割语音文件 | 分割用「编码器槽位」原子替换实现无间隙分段，命名 `..._1/_2`；默认不勾选、30 分钟；另记录 AI 转写/摘要方案与 4 个既有缺陷 | `src/recorder.*`, `src/main.cpp`, `src/localization.*`, `lang/*.ini`, `src_rs/src/app.rs`, `src_rs/src/ffi/system.rs` |
