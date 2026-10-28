# 已知坑位

## 构建与工具链

### 沙箱环境无法调用 vcvars64.bat

**现象**: CMake 无法探测到 MSVC 编译器，`CMAKE_MT-NOTFOUND`、`rc` 缺失。

**解决**: 使用 `build_release.bat` 显式设置 PATH/INCLUDE/LIB，使用 NMake Makefiles 生成器。

**相关文件**: `build_release.bat`

### ini 文件编码必须 UTF-16LE with BOM

**现象**: 使用 `GetPrivateProfileSectionW` 加载的 ini 文件中文乱码或截断。

**解决**: 改用直接读取 UTF-16LE 文件并自行解析，不再依赖 `GetPrivateProfileSectionW`。

**相关文件**: `src/localization.cpp:LoadLangFile()`

### ini 文件行尾必须 \r\n

**现象**: `GetPrivateProfileSectionW` 在 `\n`（非 `\r\n`）行尾时只解析到第一行就中止。

**解决**: 转换 ini 文件时确保 `\r\n` 行尾。

**相关文件**: `lang/zh-CN.ini`, `lang/en-US.ini`

### Rust 版忘记链接 advapi32

**现象**: `RegCreateKeyExW` 等注册表函数链接时 unresolved external symbol。

**解决**: 在 `src_rs/src/ffi/system.rs` 中添加 `#[link(name = "advapi32")]` 块。

## GUI

### EnumChildWindows 在枚举中 DestroyWindow 会跳过控件

**现象**: 语言切换时部分控件未被销毁和重建，导致旧语言的控件残留。

**解决**: 改用 `GetWindow(hwnd, GW_CHILD)` + `GW_HWNDNEXT` 循环逐个销毁。

**相关文件**: `src/main.cpp:ReloadUI()`

### Switch 语言后 checkbox 状态丢失

**现象**: 重新创建控件时 AEC 和 Teams checkbox 总是设为默认值。

**解决**: 在销毁前保存状态到 `g_aecSaved` / `g_autoTeamsSaved` / `g_autoStartSaved`。

**相关文件**: `src/main.cpp:ReloadUI()`

### WM_SYSCOMMAND 中自定义菜单 ID 的 0xFFF0 掩码问题

**现象**: `IDM_LANG_ZHCN` (3002) 和 `IDM_LANG_ENUS` (3003) 经过 `0xFFF0` 掩码后得到相同值，永远无法触发正确的语言切换。

**解决**: 自定义菜单 ID 用 `LOWORD(wp)` 直接比较，不用 `0xFFF0` 掩码。

**相关文件**: `src/main.cpp:WM_SYSCOMMAND`

### 系统菜单五项标准项 + 删除旧自定义项时不能删到标准项

**现象**: `while (GetMenuItemCount(sysMenu) > 5)` 在 `> 5` 时删除了关闭按钮，导致关闭按钮失效。

**解决**: 改为 `> 7`（保留 7 个标准项）。

**相关文件**: `src/main.cpp:RecreateControls()`

## 音频

### Windows N/KN 版本缺少 Media Feature Pack

**现象**: `AddStream(MP3)` 返回失败。

**解决**: 错误提示中指示用户安装 Media Feature Pack。

**相关文件**: `src/mp3_writer.cpp:Open()`

### AEC 步长默认值 0.25f 是死代码

**现象**: 成员初始化器设置 `m_mu = 0.25f`，但 `Configure()` 会立即覆盖为传入参数（默认 0.1f）。

**解决**: 保留不动，避免引入不必要的变更。

**相关文件**: `src/echo_canceller.h`

### 浮点数转 int16 的舍入

**现象**: `FloatToI16` 使用 `lrintf`，精度取决于 `math.h` 实现。

**影响**: 音量极低时可能出现舍入噪声，对语音录制场景影响忽略不计。

## 会议检测

### 进程名只匹配文件名，不校验完整路径

**现象**: 若有同名恶意程序可能被误判。

**影响**: 极低风险，会议 App 场景下可接受。

**相关文件**: `src/teams_watcher.cpp:IsMeetingProcess()`

### 腾讯会议可能有多个进程

**现象**: 同时匹配了 `wemeet.exe` / `wemeetapp.exe` / `wemeetlauncher.exe`，如果多个进程同时检测可能导致重复触
发。

**解决**: `MeetingActive()` 只返回 bool，不区分具体进程。

## Rust 版特有

### Rust 版 app.rs 过于庞大

**现象**: `app.rs` 约 1400 行，包含了 GUI、录制控制、日志、配置、国际化等所有逻辑。

**影响**: 与 C++ 版的 11 文件拆分形成对比，Rust 版的模块化较弱。

**建议**: 未来可拆分为 `app_gui.rs` / `app_recording.rs` / `app_locale.rs` 等。

### Rust 版全局状态使用 thread_local! + RefCell

**现象**: `APP: RefCell<App>` 通过 `thread_local!` 持有。

**影响**: `MessageBoxW` 和 `TrackPopupMenu` 会自己泵消息，期间可能导致嵌套借用。已通过 `PendingMessage` 和 `TPM_RETURNCMD` 解
决。

**相关文件**: `src_rs/src/app.rs:83-86`