//! 桌面音频录制的 GUI：主窗口与控件、系统托盘、Teams 会议看门狗。
//! 与 C++ 版 `src/main.cpp` 一一对应。
//!
//! # 关于全局状态
//!
//! C++ 版用的是一堆文件级全局变量，窗口过程直接读写它们。Rust 这边用
//! `thread_local!` + `RefCell` 表达同一件事（窗口过程只会在 UI 线程被调用），
//! 但有一点必须小心：**`MessageBoxW` 与 `TrackPopupMenu` 会自己泵消息**，
//! 期间嵌套的 `WM_TIMER` 会再次借用同一份状态。因此凡是会泵消息的调用，
//! 都在借用释放之后才执行：需要弹窗时把内容放进 [`PendingMessage`] 返回出来，
//! 托盘菜单则在 `TrackPopupMenu` 返回后统一处理（用 `TPM_RETURNCMD` 避免
//! 它再发一趟 `WM_COMMAND`）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use core::ptr::{null, null_mut};

use crate::audio_mixer::AudioMixer;
use crate::ffi::system::{
    AdjustWindowRect, AppendMenuW, CoCreateInstance, CoTaskMemFree, ComApartment, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EnableWindow,
    FindWindowW, GetCursorPos, GetLocalTime, GetMenuItemCount, GetMessageW, GetModuleFileNameW, GetModuleHandleW,
GetPrivateProfileStringW, GetStockObject,
GetSystemMenu, GetSystemMetrics, GetTickCount64, GetUserDefaultUILanguage, GetWindow, GetWindowTextW,
    InitCommonControlsEx, KillTimer, LoadCursorW, LoadImageW,
    MessageBoxW, PostMessageW, PostQuitMessage, PropVariantClear, RegisterClassExW, RemoveMenu, SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowTextW, Shell_NotifyIconW,
    ShowWindow, TrackPopupMenu, TranslateMessage, UpdateWindow, RegCreateKeyExW, RegSetValueExW,
    RegDeleteValueW, RegCloseKey, WritePrivateProfileStringW, BM_GETCHECK, BM_SETCHECK,
    BS_AUTOCHECKBOX, BST_CHECKED, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL,
    CBS_DROPDOWNLIST, COLOR_BTNFACE, CW_USEDEFAULT, DEFAULT_GUI_FONT, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, ICC_STANDARD_CLASSES, IDC_ARROW, IDC_WAIT, IMAGE_ICON, INITCOMMONCONTROLSEX, GW_CHILD, GW_HWNDNEXT,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, ERROR_SUCCESS,
    LR_DEFAULTSIZE, LR_SHARED, MAX_PATH, MB_OK, MB_ICONERROR, MB_ICONINFORMATION, MF_BYPOSITION,
    MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NOTIFYICONDATAW, POINT, RECT, SC_MINIMIZE, SM_CXSMICON, SM_CYSMICON, SS_LEFT, SW_HIDE,
    SW_RESTORE, SW_SHOW, SW_SHOWDEFAULT, ShellExecuteW, SW_SHOWNORMAL, SYSTEMTIME, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_RETURNCMD, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_SETFONT, WM_SYSCOMMAND, WM_TIMER, WNDCLASSEXW,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
    WS_VSCROLL,
};
use crate::ffi::wasapi::{
    eCapture, eConsole, eRender, EDataFlow, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator,
    IPropertyStore, CLSID_MMDeviceEnumerator, DEVICE_STATE_ACTIVE, PKEY_DEVICE_FRIENDLY_NAME,
    STGM_READ,
};
use crate::ffi::{
    failed, make_int_resource, succeeded, Com, ComPtr, DWORD, HBRUSH, HCURSOR, HICON, HKEY,
    HMENU, HWND, LPARAM, LPCWSTR, LPWSTR, LRESULT, PROPVARIANT, SendableHwnd, UINT, UINT_PTR,
    WPARAM, CLSCTX_ALL, FALSE, TRUE, VT_LPWSTR,
};
use crate::mp3_writer::Mp3Writer;
use crate::teams_watcher::TeamsWatcher;
use crate::util::{from_wide, to_wide};
use crate::wasapi_capture::WasapiCapture;

// ---- 与 C++ 版一致的控件 ID / 定时器 / 常量 ----

const IDC_COMBO_DEVICE: i32 = 1001;
const IDC_BTN_START: i32 = 1002;
const IDC_BTN_REFRESH: i32 = 1003;
const IDC_EDIT_STATUS: i32 = 1004;
const IDC_COMBO_MIC: i32 = 1005;
const IDC_CHK_AEC: i32 = 1006;
const IDC_CHK_AUTOTEAMS: i32 = 1007;
const IDC_BTN_OPENFOLDER: i32 = 1008;
const IDC_CHK_AUTOSTART: i32 = 1010;

const TIMER_ID: usize = 1;
const TIMER_WATCH: usize = 2;
const WM_RECORD_ERROR: UINT = WM_APP + 1;
const WM_TRAYICON: UINT = WM_APP + 2;
const TRAY_UID: UINT = 1;
const IDM_TRAY_OPEN: usize = 2001;
const IDM_TRAY_EXIT: usize = 2002;
const IDM_SYS_ABOUT: usize = 3001;
const IDM_LANG_ZHCN: usize = 3002;
const IDM_LANG_ENUS: usize = 3003;

/// 192 kbps
const BITRATE: u32 = 192_000;
/// 会议结束防抖：连续 5 次未检测到才停录（2s × 5 = 10s）
const WATCH_GRACE_POLLS: u32 = 5;
/// `resource.h` 里的 `IDI_APP`
const IDI_APP: u16 = 101;

static LOCALE: OnceLock<String> = OnceLock::new();
static STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();

fn detect_system_locale() -> String {
    let lang = unsafe { GetUserDefaultUILanguage() };
    if lang & 0x3FF == 0x04 {
        "zh-CN".to_string()
    } else {
        "en-US".to_string()
    }
}

fn exe_dir() -> String {
    let mut buf = [0u16; MAX_PATH];
    unsafe {
        let len = GetModuleFileNameW(null_mut(), buf.as_mut_ptr(), MAX_PATH as DWORD);
        let exe = PathBuf::from(crate::util::from_wide(&buf[..len as usize]));
        exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    }
}

fn exe_name() -> String {
    let mut buf = [0u16; MAX_PATH];
    unsafe {
        let len = GetModuleFileNameW(null_mut(), buf.as_mut_ptr(), MAX_PATH as DWORD);
        let exe = PathBuf::from(crate::util::from_wide(&buf[..len as usize]));
        exe.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }
}

fn load_lang_file(locale: &str) {
    let dir = exe_dir();
    let ini_path = format!("{}\\lang\\{}.ini", dir, locale);
    let content = match std::fs::read(&ini_path) {
        Ok(d) => {
            // UTF-16LE decode (may have BOM)
            if d.len() < 2 {
                return;
            }
            let bom_offset = if d[0] == 0xFF && d[1] == 0xFE { 2 } else { 0 };
            let u16_data = unsafe {
                std::slice::from_raw_parts(d.as_ptr().add(bom_offset) as *const u16, (d.len() - bom_offset) / 2)
            };
            let mut s = String::with_capacity(u16_data.len());
            for &c in u16_data {
                if c != 0 {
                    s.push(char::from_u32(c as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
                }
            }
            s
        }
        Err(_) => return,
    };

    let mut map = HashMap::new();
    let mut sec_name = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            sec_name = line[1..line.len() - 1].to_string();
        } else if !sec_name.is_empty() {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].to_string();
                let val = line[eq + 1..].replace("\\n", "\r\n");
                map.insert(format!("{}.{}", sec_name, key), val);
            }
        }
    }

    let _ = STRINGS.set(map);
}

fn load_config() {
    let dir = exe_dir();
    let name = exe_name();
    let ini_path = format!("{}\\{}.ini", dir, name);
    let ini_wide = crate::util::to_wide(&ini_path);
    let key_wide = crate::util::to_wide("locale");
    let sec_wide = crate::util::to_wide("Language");
    let default_wide = crate::util::to_wide("");
    let mut buf = [0u16; 16];
    let opts_wide = crate::util::to_wide("Options");
    let kt_wide = crate::util::to_wide("auto_teams");
    let ks_wide = crate::util::to_wide("auto_start");
    let zero_wide = crate::util::to_wide("0");
    unsafe {
        GetPrivateProfileStringW(sec_wide.as_ptr(), key_wide.as_ptr(), default_wide.as_ptr(), buf.as_mut_ptr(), 16, ini_wide.as_ptr());
    }
    let locale = if buf[0] == 0 {
        detect_system_locale()
    } else {
        crate::util::from_wide(&buf)
    };
    let _ = LOCALE.set(locale.clone());
    load_lang_file(&locale);

    unsafe {
        GetPrivateProfileStringW(opts_wide.as_ptr(), kt_wide.as_ptr(), zero_wide.as_ptr(), buf.as_mut_ptr(), 8, ini_wide.as_ptr());
    }
    let auto_teams = buf[0] == '1' as u16;
    unsafe {
        GetPrivateProfileStringW(opts_wide.as_ptr(), ks_wide.as_ptr(), zero_wide.as_ptr(), buf.as_mut_ptr(), 8, ini_wide.as_ptr());
    }
    let auto_start = buf[0] == '1' as u16;
    APP.with(|cell| {
        let mut app = cell.borrow_mut();
        app.auto_teams_checked = auto_teams;
        app.auto_start_checked = auto_start;
    });
}

fn save_config(locale: &str) {
    let dir = exe_dir();
    let name = exe_name();
    let ini_path = format!("{}\\{}.ini", dir, name);
    let ini_wide = crate::util::to_wide(&ini_path);
    unsafe {
        let sec = crate::util::to_wide("Language");
        let key = crate::util::to_wide("locale");
        let val = crate::util::to_wide(locale);
        WritePrivateProfileStringW(sec.as_ptr(), key.as_ptr(), val.as_ptr(), ini_wide.as_ptr());

        let opts = crate::util::to_wide("Options");
        let k1 = crate::util::to_wide("auto_teams");
        let k2 = crate::util::to_wide("auto_start");
        let v1 = crate::util::to_wide(if APP.with(|c| c.borrow().auto_teams_checked) { "1" } else { "0" });
        let v2 = crate::util::to_wide(if APP.with(|c| c.borrow().auto_start_checked) { "1" } else { "0" });
        WritePrivateProfileStringW(opts.as_ptr(), k1.as_ptr(), v1.as_ptr(), ini_wide.as_ptr());
        WritePrivateProfileStringW(opts.as_ptr(), k2.as_ptr(), v2.as_ptr(), ini_wide.as_ptr());
    }
}

fn tr(section: &str, key: &str) -> &'static str {
    let full_key = format!("{}.{}", section, key);
    STRINGS.get()
        .and_then(|m| m.get(&full_key))
        .map(|s| s.as_str())
        .unwrap_or("?")
}

thread_local! {
    /// UI 线程独占的应用状态（窗口过程只会在 UI 线程被调用）。
    static APP: RefCell<App> = RefCell::new(App::new());
}

// ---------------------------------------------------------------------------
// 需要在借用释放后再显示的消息框
// ---------------------------------------------------------------------------

struct PendingMessage {
    text: String,
    title: String,
    flags: UINT,
}

impl PendingMessage {
    fn error(text: String) -> Self {
        Self { text, title: tr("Status", "interrupted").to_string(), flags: MB_ICONERROR }
    }

    fn recording_error(text: String) -> Self {
        Self { text, title: format!("{} {}", tr("Status", "recording"), tr("Status", "interrupted")), flags: MB_ICONERROR }
    }

    fn info(text: String, title: &str) -> Self {
        Self { text, title: title.to_string(), flags: MB_ICONINFORMATION }
    }

    /// 必须在**没有**持有 `APP` 借用时调用。
    fn show(&self, hwnd: HWND) {
        let text = to_wide(&self.text);
        let title = to_wide(&self.title);
        // Safety: 两个字符串都是 NUL 结尾的 UTF-16，生命周期覆盖本次调用。
        unsafe { MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), self.flags) };
    }
}

// ---------------------------------------------------------------------------
// 一次录制会话
// ---------------------------------------------------------------------------

struct Session {
    desktop: WasapiCapture,
    mic: Option<WasapiCapture>,
    mixer: Arc<AudioMixer>,
    writer: Arc<Mp3Writer>,
}

impl Session {
    /// 停止采集、冲刷编码器；返回 `(是否出错, 错误描述)`。
    fn stop(mut self) -> (bool, String) {
        if let Some(mic) = self.mic.take() {
            mic.stop();
        }
        self.desktop.stop();
        self.writer.close();
        // 混音器持有编码器的回调（Arc），先丢掉混音器再读编码器状态，
        // 保证读到的是一致的结果（与 C++ 版 reset 顺序一致）。
        drop(self.mixer);
        (self.writer.has_error(), self.writer.error())
    }
}

/// `start_recording` 的结果。
struct StartOutcome {
    started: bool,
    /// 非自动模式下失败时需要弹出的消息框。
    message: Option<PendingMessage>,
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

struct App {
    hwnd: HWND,
    combo: HWND,
    combo_mic: HWND,
    btn_start: HWND,
    btn_refresh: HWND,
    btn_open_folder: HWND,
    record_count_text: HWND,
    record_count: i32,
    status: HWND,
    chk_aec: HWND,
    chk_auto_teams: HWND,
    chk_auto_start: HWND,
    aec_checked: bool,
    auto_teams_checked: bool,
    auto_start_checked: bool,

    /// 与播放下拉框索引一一对应
    render_ids: Vec<Vec<u16>>,
    /// 麦克风下拉框：索引 0 = 不录制，之后对应此数组
    mic_ids: Vec<Vec<u16>>,

    session: Option<Session>,
    in_tray: bool,
    /// 当前录制由 Teams 看门狗自动触发
    auto_started: bool,
    /// 用户手动停止了自动录制：同一场会议内不再自动开始
    auto_suppress: bool,
    /// 会议结束防抖计数
    inactive_polls: u32,
    start_tick: u64,
    start_time: String,
    out_path: PathBuf,
    device_name: String,
    mic_name: String,
}

impl App {
    fn new() -> Self {
        Self {
            hwnd: null_mut(),
            combo: null_mut(),
            combo_mic: null_mut(),
            btn_start: null_mut(),
            btn_refresh: null_mut(),
            btn_open_folder: null_mut(),
            record_count_text: null_mut(),
            record_count: 0,
            status: null_mut(),
            chk_aec: null_mut(),
            chk_auto_teams: null_mut(),
            chk_auto_start: null_mut(),
            aec_checked: true,
            auto_teams_checked: false,
            auto_start_checked: false,
            render_ids: Vec::new(),
            mic_ids: Vec::new(),
            session: None,
            in_tray: false,
            auto_started: false,
            auto_suppress: false,
            inactive_polls: 0,
            start_tick: 0,
            start_time: String::new(),
            out_path: PathBuf::new(),
            device_name: String::new(),
            mic_name: String::new(),
        }
    }

    // ---- 控件 ----

    fn set_status(&mut self, text: &str) {
        set_window_text(self.status, text);
    }

    fn recreate_controls(&mut self, hwnd: HWND) {
        unsafe {
            let font = GetStockObject(DEFAULT_GUI_FONT);

            let make = |class: &str, text: &str, style: DWORD, x: i32, y: i32, w: i32, h: i32, id: i32| -> HWND {
                let cls = to_wide(class);
                let txt = to_wide(text);
                let control = CreateWindowExW(
                    0,
                    cls.as_ptr(),
                    txt.as_ptr(),
                    style | WS_CHILD | WS_VISIBLE,
                    x,
                    y,
                    w,
                    h,
                    hwnd,
                    id as usize as HMENU,
                    null_mut(),
                    null_mut(),
                );
                SendMessageW(control, WM_SETFONT, font as WPARAM, TRUE as LPARAM);
                control
            };

            make("STATIC", tr("UI", "label_playback"), SS_LEFT, 12, 16, 64, 18, -1);
            self.combo = make(
                "COMBOBOX",
                "",
                CBS_DROPDOWNLIST | WS_VSCROLL,
                80,
                12,
                352,
                300,
                IDC_COMBO_DEVICE,
            );
            self.btn_refresh = make("BUTTON", tr("UI", "btn_refresh"), 0, 440, 11, 68, 25, IDC_BTN_REFRESH);
            make("STATIC", tr("UI", "label_mic"), SS_LEFT, 12, 50, 64, 18, -1);
            self.combo_mic = make(
                "COMBOBOX",
                "",
                CBS_DROPDOWNLIST | WS_VSCROLL,
                80,
                46,
                352,
                300,
                IDC_COMBO_MIC,
            );
            if self.session.is_some() {
                self.btn_start = make("BUTTON", tr("UI", "btn_stop"), 0, 12, 84, 130, 25, IDC_BTN_START);
            } else {
                self.btn_start = make("BUTTON", tr("UI", "btn_start"), 0, 12, 84, 130, 25, IDC_BTN_START);
            }
            let count_text = format!("{}: {}", tr("UI", "record_count"), self.record_count);
            self.record_count_text = make("STATIC", &count_text, SS_LEFT, 146, 88, 70, 18, -1);
            self.btn_open_folder = make("BUTTON", tr("UI", "btn_open_folder"), 0, 220, 84, 100, 25, IDC_BTN_OPENFOLDER);
            self.chk_auto_start = make("BUTTON", tr("UI", "chk_autostart"), BS_AUTOCHECKBOX, 326, 84, 120, 25, IDC_CHK_AUTOSTART);
            button_set_check(self.chk_auto_start, self.auto_start_checked);
            self.chk_aec = make(
                "BUTTON",
                tr("UI", "chk_aec"),
                BS_AUTOCHECKBOX,
                12,
                118,
                160,
                22,
                IDC_CHK_AEC,
            );
            button_set_check(self.chk_aec, self.aec_checked);
            self.chk_auto_teams = make(
                "BUTTON",
                tr("UI", "chk_auto_teams"),
                BS_AUTOCHECKBOX,
                180,
                118,
                220,
                22,
                IDC_CHK_AUTOTEAMS,
            );
            button_set_check(self.chk_auto_teams, self.auto_teams_checked);

            let edit_class = to_wide("EDIT");
            let empty = to_wide("");
            self.status = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                edit_class.as_ptr(),
                empty.as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL,
                12,
                148,
                496,
                110,
                hwnd,
                IDC_EDIT_STATUS as usize as HMENU,
                null_mut(),
                null_mut(),
            );
            SendMessageW(self.status, WM_SETFONT, font as WPARAM, TRUE as LPARAM);

            self.set_status(&format!(
                "{}\r\n{}\r\n{}",
                tr("Status", "ready"),
                tr("Status", "initial_help"),
                tr("Status", "legal_warning")
            ));

            // System menu
            let sys_menu = GetSystemMenu(hwnd, FALSE);
            let mut n_count = GetMenuItemCount(sys_menu);
            while n_count > 7 {
                RemoveMenu(sys_menu, (n_count - 1) as UINT, MF_BYPOSITION);
                n_count -= 1;
            }
            AppendMenuW(sys_menu, MF_SEPARATOR, 0, null_mut());
            let about_text = to_wide(tr("Menu", "sys_about"));
            AppendMenuW(sys_menu, MF_STRING, IDM_SYS_ABOUT as UINT_PTR, about_text.as_ptr());

            let lang_menu = CreatePopupMenu();
            let locale = LOCALE.get().map(|s| s.as_str()).unwrap_or("en-US");
            let zh_text = to_wide("中文 (简体)");
            let en_text = to_wide("English");
            let zh_flags = MF_STRING | if locale == "zh-CN" { MF_CHECKED } else { 0 };
            let en_flags = MF_STRING | if locale == "en-US" { MF_CHECKED } else { 0 };
            AppendMenuW(lang_menu, zh_flags, IDM_LANG_ZHCN as UINT_PTR, zh_text.as_ptr());
            AppendMenuW(lang_menu, en_flags, IDM_LANG_ENUS as UINT_PTR, en_text.as_ptr());
            let lang_label = to_wide(tr("Menu", "sys_language"));
            AppendMenuW(sys_menu, MF_POPUP, lang_menu as UINT_PTR, lang_label.as_ptr());
        }
    }

    fn on_create(&mut self, hwnd: HWND) {
        unsafe {
            self.hwnd = hwnd;
            self.recreate_controls(hwnd);
            self.refresh_devices();
            SetTimer(hwnd, TIMER_WATCH, 2000, null_mut());
        }
    }

    fn refresh_devices(&mut self) {
        unsafe { EnableWindow(self.btn_start, FALSE) };

        let (render_ids, render_status) = unsafe { fill_device_combo(self.combo, eRender, false) };
        self.render_ids = render_ids;
        let (mic_ids, mic_status) = unsafe { fill_device_combo(self.combo_mic, eCapture, true) };
        self.mic_ids = mic_ids;

        if let Some(msg) = render_status.or(mic_status) {
            self.set_status(&msg);
        }
        if !self.render_ids.is_empty() {
            unsafe { EnableWindow(self.btn_start, TRUE) };
        }
    }

    fn on_auto_teams_toggled(&mut self) {
        if button_checked(self.chk_auto_teams) {
            self.auto_suppress = false;
            self.inactive_polls = 0;
            append_log("Teams monitor ON");
            if self.session.is_none() {
                self.set_status(tr("Status", "auto_teams_on"));
            }
        } else {
            append_log("Teams monitor OFF");
        }
    }

    // ---- 录制 ----

    fn show_recording_status(&mut self) {
        let elapsed = unsafe { GetTickCount64() } - self.start_tick;
        let text = format!(
            "{}: {}\r\n{}  {} {}\r\n{}: {}\r\n{}: {}\r\n{}: {}",
            tr("Status", "start"),
            self.start_time,
            tr("Status", "recording"),
            tr("Status", "duration"),
            format_duration(elapsed),
            tr("UI", "label_playback"),
            self.device_name,
            tr("UI", "label_mic"),
            self.mic_name,
            tr("Status", "file"),
            self.out_path.display()
        );
        self.set_status(&text);
    }

    /// 失败处理：自动模式下只写状态栏（看门狗场景避免反复弹窗），否则返回消息框。
    fn fail(&mut self, from_auto: bool, msg: String) -> Option<PendingMessage> {
        if from_auto {
            self.set_status(&msg);
            None
        } else {
            Some(PendingMessage::error(msg))
        }
    }

    /// `start_recording` 的失败出口。
    fn start_failure(&mut self, from_auto: bool, msg: String) -> StartOutcome {
        StartOutcome { started: false, message: self.fail(from_auto, msg) }
    }

    fn start_recording(&mut self, from_auto: bool) -> StartOutcome {
        let sel = combo_cur_sel(self.combo);
if sel < 0 || sel as usize >= self.render_ids.len() {
            return StartOutcome {
                started: false,
                message: if from_auto {
                    None
                } else {
                    Some(PendingMessage::info(tr("Status", "error_no_device").to_string(), tr("About", "title")))
                },
            };
        }
        // 0 = 不录制
        let mic_sel = combo_cur_sel(self.combo_mic);

        self.device_name = combo_text(self.combo);
        self.mic_name = tr("UI", "mic_none").to_string();

        // 1) 桌面 loopback 采集
        let device_id = self.render_ids[sel as usize].clone();
        let mut desktop = WasapiCapture::new();
        if let Err(e) = desktop.start(Some(&device_id), true) {
            return self.start_failure(from_auto, format!("{}\r\n{e}", tr("Status", "error_capture")));
        }

        // 2) 麦克风采集（可选）
        let mut mic: Option<WasapiCapture> = None;
        if mic_sel > 0 && (mic_sel as usize - 1) < self.mic_ids.len() {
            let mic_id = self.mic_ids[mic_sel as usize - 1].clone();
            let mut cap = WasapiCapture::new();
            if let Err(e) = cap.start(Some(&mic_id), false) {
                desktop.stop();
                return self.start_failure(
                    from_auto,
                    format!("{}\r\n{e}\r\n{}", tr("Status", "error_mic"), tr("Status", "error_mic_hint")),
                );
            }
            self.mic_name = combo_text(self.combo_mic);
            mic = Some(cap);
        }

        // 3) 混音器 + 编码器（输出格式跟随桌面流）
        let rate = desktop.sample_rate();
        let channels = desktop.channels();
        self.out_path = make_output_path();

        let writer = match Mp3Writer::open(&self.out_path, rate, channels, BITRATE) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                if let Some(m) = mic.as_ref() {
                    m.stop();
                }
                desktop.stop();
                return self.start_failure(
                    from_auto,
                    format!("{}\r\n{e}\r\n{}", tr("Status", "error_mp3"), tr("Status", "error_mp3_hint")),
                );
            }
        };

        let mixer = Arc::new(AudioMixer::new());
        {
            let writer_for_mix = Arc::clone(&writer);
            mixer.open(
                rate,
                channels,
                Box::new(move |data: &[i16]| writer_for_mix.push(data)),
            );
        }
        mixer.set_echo_cancellation(button_checked(self.chk_aec));
        if let Some(m) = mic.as_ref() {
            mixer.set_mic_format(m.sample_rate(), m.channels());
        }

        // 采集线程的运行期错误 -> 投递到 UI 线程（HWND 只是一个标识，跨线程 PostMessage 是安全的）
        let desktop_error_hwnd = SendableHwnd(self.hwnd);
        {
            let mix_desktop = Arc::clone(&mixer);
            desktop.begin(
                Box::new(move |data: &[i16]| mix_desktop.push_desktop(data)),
                Box::new(move |msg: String| post_record_error(desktop_error_hwnd, msg)),
            );
        }
        if let Some(m) = mic.as_mut() {
            let mic_error_hwnd = SendableHwnd(self.hwnd);
            let mix_mic = Arc::clone(&mixer);
            m.begin(
                Box::new(move |data: &[i16]| mix_mic.push_mic(data)),
                Box::new(move |msg: String| post_record_error(mic_error_hwnd, msg)),
            );
        }

        self.session = Some(Session { desktop, mic, mixer, writer });
        self.start_tick = unsafe { GetTickCount64() };
        self.start_time = timestamp();
        append_log(&format!("Start  device: {}  mic: {}  file: {}", self.device_name, self.mic_name, self.out_path.display()));
        unsafe { SetTimer(self.hwnd, TIMER_ID, 500, null_mut()) };

        set_window_text(self.btn_start, tr("UI", "btn_stop"));
        unsafe {
            EnableWindow(self.combo, FALSE);
            EnableWindow(self.combo_mic, FALSE);
            EnableWindow(self.chk_aec, FALSE);
            EnableWindow(self.btn_refresh, FALSE);
        }
        self.show_recording_status();

        StartOutcome { started: true, message: None }
    }

    fn stop_recording(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        unsafe { KillTimer(self.hwnd, TIMER_ID) };

        let old_cursor = unsafe { SetCursor(LoadCursorW(null_mut(), IDC_WAIT as usize as LPCWSTR)) };
        let (has_error, error) = session.stop();
        unsafe { SetCursor(old_cursor) };

        let end_time = timestamp();
        let status = if has_error {
            format!(
                "{}: {}\r\n{}: {}\r\n{}：{error}\r\n{}: {}",
                tr("Status", "start"),
                self.start_time,
                tr("Status", "end"),
                end_time,
                tr("Status", "interrupted"),
                tr("Status", "file"),
                self.out_path.display()
            )
        } else {
            let size = std::fs::metadata(&self.out_path)
                .map(|m| format!("{:.1} MB", m.len() as f64 / 1_048_576.0))
                .unwrap_or_else(|_| "?".to_string());
            let elapsed = unsafe { GetTickCount64() } - self.start_tick;
            format!(
                "{}: {}\r\n{}: {}\r\n{}  {}  {}  {}\r\n{}: {}",
                tr("Status", "start"),
                self.start_time,
                tr("Status", "end"),
                end_time,
                tr("Status", "completed"),
                format_duration(elapsed),
                tr("Status", "size"),
                size,
                tr("Status", "file"),
                self.out_path.display()
            )
        };
        self.set_status(&status);
        let log_line = if has_error {
            format!("{}：{error}  {}: {}", tr("Status", "interrupted"), tr("Status", "file"), self.out_path.display())
        } else {
            let elapsed = unsafe { GetTickCount64() } - self.start_tick;
            format!("{}  {}  {}: {}", tr("Status", "completed"), format_duration(elapsed), tr("Status", "file"), self.out_path.display())
        };
        append_log(&log_line);

        // 用户手动停止了自动开始的录制 → 本次会议内抑制自动重开
        if self.auto_started {
            self.auto_started = false;
            if button_checked(self.chk_auto_teams) && TeamsWatcher::meeting_active() {
                self.auto_suppress = true;
            }
        }

        self.record_count += 1;
        set_window_text(self.record_count_text, &format!("{}: {}", tr("UI", "record_count"), self.record_count));

        set_window_text(self.btn_start, tr("UI", "btn_start"));
        unsafe {
            EnableWindow(self.combo, TRUE);
            EnableWindow(self.combo_mic, TRUE);
            EnableWindow(self.chk_aec, TRUE);
            EnableWindow(self.btn_refresh, TRUE);
        }
    }

    /// Teams 会议看门狗：勾选后每 2s 检测一次。
    /// 检测到会议 → 自动开始录制；会议消失持续 10s（防抖）→ 自动停止。
    fn on_watch_tick(&mut self) {
        if !button_checked(self.chk_auto_teams) {
            return;
        }

        if TeamsWatcher::meeting_active() {
            self.inactive_polls = 0;
            if self.session.is_none() && !self.auto_suppress {
                if self.start_recording(true).started {
                    self.auto_started = true;
                } else {
                    // 自动开始失败：本次会议内不再反复尝试
                    self.auto_suppress = true;
                }
            }
        } else {
            self.auto_suppress = false;
            if self.session.is_some() && self.auto_started {
                self.inactive_polls += 1;
                if self.inactive_polls >= WATCH_GRACE_POLLS {
                    append_log(&format!("Teams meeting ended, auto stop  file: {}", self.out_path.display()));
                    self.stop_recording();
                    self.inactive_polls = 0;
                    let path = self.out_path.display().to_string();
                    self.set_status(&format!("{}: {}\r\n{}: {}\r\n{}\r\n{}: {path}", tr("Status", "start"), self.start_time, tr("Status", "end"), timestamp(), tr("Status", "teams_stopped"), tr("Status", "file")));
                }
            } else {
                self.inactive_polls = 0;
            }
        }
    }

    // ---- 系统托盘 ----

    fn minimize_to_tray(&mut self) {
        unsafe {
            let mut nid = NOTIFYICONDATAW::zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as DWORD;
            nid.hWnd = self.hwnd;
            nid.uID = TRAY_UID;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAYICON;

            let module = GetModuleHandleW(null_mut());
            nid.hIcon = LoadImageW(
                module,
                make_int_resource(IDI_APP),
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_SHARED,
            );
            let mut tip = tr("UI", "app_title").to_string();
            if self.session.is_some() {
                tip.push_str(&format!("（{}）", tr("Status", "recording")));
            }
            let tip_wide = to_wide(&tip);
            let n = tip_wide.len().min(nid.szTip.len());
            nid.szTip[..n].copy_from_slice(&tip_wide[..n]);

            Shell_NotifyIconW(NIM_ADD, &mut nid);
            ShowWindow(self.hwnd, SW_HIDE);
            self.in_tray = true;
        }
    }

    fn remove_tray_icon(&mut self) {
        if !self.in_tray {
            return;
        }
        unsafe {
            let mut nid = NOTIFYICONDATAW::zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as DWORD;
            nid.hWnd = self.hwnd;
            nid.uID = TRAY_UID;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
        }
        self.in_tray = false;
    }

    fn restore_from_tray(&mut self) {
        if !self.in_tray {
            return;
        }
        self.remove_tray_icon();
        unsafe {
            ShowWindow(self.hwnd, SW_SHOW);
            ShowWindow(self.hwnd, SW_RESTORE);
            SetForegroundWindow(self.hwnd);
        }
    }
}

// ---------------------------------------------------------------------------
// 控件辅助
// ---------------------------------------------------------------------------

fn set_window_text(hwnd: HWND, text: &str) {
    let wide = to_wide(text);
    unsafe { SetWindowTextW(hwnd, wide.as_ptr()) };
}

fn combo_add_string(combo: HWND, text: &str) -> i32 {
    let wide = to_wide(text);
    unsafe { SendMessageW(combo, CB_ADDSTRING, 0, wide.as_ptr() as LPARAM) as i32 }
}

fn combo_cur_sel(combo: HWND) -> i32 {
    unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) as i32 }
}

fn combo_set_cur_sel(combo: HWND, index: i32) {
    unsafe { SendMessageW(combo, CB_SETCURSEL, index as WPARAM, 0) };
}

/// 取下拉框当前选中项的文本（`CBS_DROPDOWNLIST` 下即选中项文字）。
fn combo_text(combo: HWND) -> String {
    let mut buf = [0u16; 256];
    unsafe { GetWindowTextW(combo, buf.as_mut_ptr(), buf.len() as i32) };
    from_wide(&buf)
}

fn button_checked(button: HWND) -> bool {
    unsafe { SendMessageW(button, BM_GETCHECK, 0, 0) as usize == BST_CHECKED }
}

fn button_set_check(button: HWND, checked: bool) {
    unsafe {
        SendMessageW(
            button,
            BM_SETCHECK,
            if checked { BST_CHECKED } else { 0 },
            0,
        )
    };
}

/// 采集线程的运行期错误 -> `WM_RECORD_ERROR`（`lParam` 是 `Box<String>` 的裸指针）。
fn post_record_error(hwnd: SendableHwnd, msg: String) {
    let payload = Box::into_raw(Box::new(msg));
    unsafe { PostMessageW(hwnd.0, WM_RECORD_ERROR, 0, payload as LPARAM) };
}

/// 复制一个 `LPWSTR`（含结尾 NUL）成 `Vec<u16>`。
///
/// # Safety
/// `p` 必须指向合法的 NUL 结尾 UTF-16 字符串。
unsafe fn wide_string_from_ptr(p: LPWSTR) -> Vec<u16> {
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::slice::from_raw_parts(p, len + 1).to_vec()
}

/// 枚举音频端点填充下拉框；`flow = eRender` 播放设备，`eCapture` 录音设备。
/// `add_none = true` 时在索引 0 插入"（不录制）"项。
///
/// 返回 `(设备 ID 列表, 需要显示到状态栏的提示)`。
///
/// # Safety
/// 调用线程必须已初始化 COM，且 `combo` 是合法的下拉框句柄。
unsafe fn fill_device_combo(
    combo: HWND,
    flow: EDataFlow,
    add_none: bool,
) -> (Vec<Vec<u16>>, Option<String>) {
    SendMessageW(combo, CB_RESETCONTENT, 0, 0);
    let mut ids: Vec<Vec<u16>> = Vec::new();

    let base = if add_none {
        combo_add_string(combo, tr("UI", "mic_none"));
        1
    } else {
        0
    };

    let mut enumerator = ComPtr::<IMMDeviceEnumerator>::null();
    let hr = CoCreateInstance(
        &CLSID_MMDeviceEnumerator,
        null_mut(),
        CLSCTX_ALL,
        &IMMDeviceEnumerator::IID,
        enumerator.as_void_out_ptr(),
    );
    if failed(hr) {
        if add_none {
            combo_set_cur_sel(combo, 0);
        }
        return (ids, Some(tr("Status", "error_capture").to_string()));
    }
    let enumerator_ref = enumerator.expect_ref();

    // 默认设备（用于默认选中项）
    let mut default_id: Vec<u16> = Vec::new();
    let mut default_device = ComPtr::<IMMDevice>::null();
    if succeeded(enumerator_ref.get_default_audio_endpoint(
        flow,
        eConsole,
        default_device.as_out_ptr(),
    )) {
        let mut id_ptr: LPWSTR = null_mut();
        if succeeded(default_device.expect_ref().get_id(&mut id_ptr)) && !id_ptr.is_null() {
            default_id = wide_string_from_ptr(id_ptr);
            CoTaskMemFree(id_ptr as *mut core::ffi::c_void);
        }
    }

    let mut select_index: i32 = if add_none { 0 } else { -1 };

    let mut collection = ComPtr::<IMMDeviceCollection>::null();
    if succeeded(enumerator_ref.enum_audio_endpoints(
        flow,
        DEVICE_STATE_ACTIVE,
        collection.as_out_ptr(),
    )) {
        let collection_ref = collection.expect_ref();
        let mut count: UINT = 0;
        collection_ref.get_count(&mut count);

        for i in 0..count {
            let mut device = ComPtr::<IMMDevice>::null();
            if failed(collection_ref.item(i, device.as_out_ptr())) {
                continue;
            }
            let device_ref = device.expect_ref();

            let mut id_ptr: LPWSTR = null_mut();
            device_ref.get_id(&mut id_ptr);
            let id = if id_ptr.is_null() {
                Vec::new()
            } else {
                let v = wide_string_from_ptr(id_ptr);
                CoTaskMemFree(id_ptr as *mut core::ffi::c_void);
                v
            };

            // 友好名来自属性存储
            let mut name = String::new();
            let mut store = ComPtr::<IPropertyStore>::null();
            if succeeded(device_ref.open_property_store(STGM_READ, store.as_out_ptr())) {
                let mut pv = PROPVARIANT::zeroed();
                if succeeded(store.expect_ref().get_value(&PKEY_DEVICE_FRIENDLY_NAME, &mut pv))
                    && pv.vt == VT_LPWSTR
                    && !pv.data.pwsz_val.is_null()
                {
                    name = from_wide(&wide_string_from_ptr(pv.data.pwsz_val));
                }
                PropVariantClear(&mut pv);
            }

            let label = if name.is_empty() { "?" } else { name.as_str() };
            let index = combo_add_string(combo, label);
            if !id.is_empty() && id == default_id {
                select_index = index;
            }
            ids.push(id);
        }
    }

    let status = if ids.is_empty() && !add_none {
        if flow == eRender {
            Some(tr("Status", "error_no_playback").to_string())
        } else {
            None
        }
    } else {
        if select_index < 0 {
            // 无默认设备时选第一个
            select_index = base;
        }
        None
    };
    combo_set_cur_sel(combo, select_index);

    (ids, status)
}

/// 输出文件路径：程序所在目录 + `recording-YYYYMMDD-HHMMSS.mp3`。
fn make_output_path() -> PathBuf {
    let mut exe_path = [0u16; MAX_PATH];
    let dir = unsafe {
        let len = GetModuleFileNameW(null_mut(), exe_path.as_mut_ptr(), MAX_PATH as DWORD);
        let exe = PathBuf::from(from_wide(&exe_path[..len as usize]));
        exe.parent().map(Path::to_path_buf).unwrap_or_default()
    };

    let mut st = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut st) };
    let name = format!(
        "recording-{:04}{:02}{:02}-{:02}{:02}{:02}.mp3",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    );
    dir.join(name)
}

fn timestamp() -> String {
    let mut st = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut st) };
    format!("[{:04}-{:02}-{:02} {:02}:{:02}:{:02}]", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
}

fn set_auto_start(on: bool) {
    let mut buf = [0u16; MAX_PATH];
    unsafe {
        let len = GetModuleFileNameW(null_mut(), buf.as_mut_ptr(), MAX_PATH as DWORD);
        let exe_path = crate::util::from_wide(&buf[..len as usize]);

        let sub_key = crate::util::to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let val_name = crate::util::to_wide("DesktopAudioRecorder");
        let mut hkey: HKEY = null_mut();
        if RegCreateKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, null_mut(), 0, KEY_SET_VALUE,
                           null_mut(), &mut hkey, null_mut()) == ERROR_SUCCESS {
            if on {
                let val = crate::util::to_wide(&exe_path);
                RegSetValueExW(hkey, val_name.as_ptr(), 0, REG_SZ,
                               val.as_ptr() as *const u8,
                               ((exe_path.len() + 1) * 2) as DWORD);
            } else {
                RegDeleteValueW(hkey, val_name.as_ptr());
            }
            RegCloseKey(hkey);
        }
    }
}

fn append_log(line: &str) {
    let mut exe_path = [0u16; MAX_PATH];
    let exe: PathBuf;
    unsafe {
        let len = GetModuleFileNameW(null_mut(), exe_path.as_mut_ptr(), MAX_PATH as DWORD);
        exe = PathBuf::from(from_wide(&exe_path[..len as usize]));
    }
    let st = timestamp();
    let log_name = format!("{}-{:04}{:02}.log", exe.file_stem().unwrap_or_default().to_string_lossy(), &st[1..5], &st[6..8]);
    let log_path = exe.parent().unwrap_or(Path::new(".")).join(log_name);

    let need_bom = !log_path.exists() || std::fs::metadata(&log_path).map(|m| m.len() == 0).unwrap_or(true);
    use std::io::Write;
    let f = std::fs::OpenOptions::new().create(true).append(!need_bom).write(true).open(&log_path);
    if let Ok(mut f) = f {
        if need_bom {
            let _ = f.write_all(&[0xEF, 0xBB, 0xBF]);
        }
        let _ = writeln!(f, "{} {}", st, line);
    }
}

/// `mm:ss` 或 `h:mm:ss`，与 C++ 版的 `FormatDuration` 一致。
fn format_duration(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

// ---------------------------------------------------------------------------
// 托盘菜单
// ---------------------------------------------------------------------------

/// 显示托盘右键菜单。**调用时不能持有 `APP` 借用**：`TrackPopupMenu` 会泵消息。
unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let open = to_wide(tr("Menu", "tray_open"));
    let exit = to_wide(tr("Menu", "tray_exit"));
    AppendMenuW(menu, MF_STRING, IDM_TRAY_OPEN, open.as_ptr());
    AppendMenuW(menu, MF_STRING, IDM_TRAY_EXIT, exit.as_ptr());

    let mut pt = POINT::default();
    GetCursorPos(&mut pt);
    // 保证菜单点击别处能自动关闭
    SetForegroundWindow(hwnd);

    // TPM_RETURNCMD：直接拿到被点中的项，避免它再发一次 WM_COMMAND
    // （那样会在菜单的嵌套消息循环里重入窗口过程）。
    let command = TrackPopupMenu(
        menu,
        TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    ) as usize;
    DestroyMenu(menu);

    match command {
        IDM_TRAY_OPEN => APP.with(|cell| cell.borrow_mut().restore_from_tray()),
        IDM_TRAY_EXIT => {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 窗口过程
// ---------------------------------------------------------------------------

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            APP.with(|cell| cell.borrow_mut().on_create(hwnd));
            0
        }

        WM_COMMAND => {
            let id = (wp & 0xFFFF) as i32;
            let pending = APP.with(|cell| {
                let mut app = cell.borrow_mut();
                match id {
                    IDC_BTN_START => {
                        if app.session.is_some() {
                            app.stop_recording();
                            None
                        } else {
                            app.start_recording(false).message
                        }
                    }
                    IDC_BTN_REFRESH => {
                        if app.session.is_none() {
                            app.refresh_devices();
                            app.set_status(tr("Status", "devices_refreshed"));
                        }
                        None
                    }
                    IDC_CHK_AUTOTEAMS => {
                        app.on_auto_teams_toggled();
                        None
                    }
                    IDC_CHK_AUTOSTART => {
                        app.auto_start_checked = button_checked(app.chk_auto_start);
                        append_log(if app.auto_start_checked { "Auto start ON" } else { "Auto start OFF" });
                        set_auto_start(app.auto_start_checked);
                        save_config(LOCALE.get().map(|s| s.as_str()).unwrap_or("en-US"));
                        None
                    }
                    IDC_BTN_OPENFOLDER => {
                        drop(app);
                        let mut exe_path = [0u16; MAX_PATH];
                        let len = GetModuleFileNameW(null_mut(), exe_path.as_mut_ptr(), MAX_PATH as DWORD);
                        let dir = PathBuf::from(from_wide(&exe_path[..len as usize]));
                        if let Some(parent) = dir.parent() {
                            let dir_wide = to_wide(&parent.to_string_lossy());
                            let open_wide = to_wide("open");
                            ShellExecuteW(hwnd, open_wide.as_ptr(), dir_wide.as_ptr(), null_mut(), null_mut(), SW_SHOWNORMAL);
                        }
                        None
                    }
                    _ => None,
                }
            });
            // 借用已释放，此时弹窗（会泵消息）是安全的
            if let Some(message) = pending {
                message.show(hwnd);
            }
            0
        }

        WM_SYSCOMMAND => {
            let cmd = (wp & 0xFFF0) as usize;
            if cmd == SC_MINIMIZE {
                APP.with(|cell| cell.borrow_mut().minimize_to_tray());
                return 0;
            }
            let id = (wp & 0xFFFF) as usize;
            if id == IDM_SYS_ABOUT {
                let about = format!(
                    "{}\r\n\r\n{}\r\n{}\r\n\r\n{}\r\n{}\r\n{}\r\n{}",
                    tr("About", "line1"),
                    tr("About", "line2"),
                    tr("About", "line3"),
                    tr("About", "line4"),
                    tr("About", "line5"),
                    tr("About", "line6"),
                    tr("About", "line7"),
                );
                let text = to_wide(&about);
                let title = to_wide(tr("About", "title"));
                MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
                return 0;
            }
            if id == IDM_LANG_ZHCN || id == IDM_LANG_ENUS {
                let new_locale = if id == IDM_LANG_ZHCN { "zh-CN" } else { "en-US" };
                save_config(new_locale);
                let _ = LOCALE.set(new_locale.to_string());
                load_lang_file(new_locale);
                APP.with(|cell| {
                    let mut app = cell.borrow_mut();
                    // Save checkbox states before destroying
                    app.aec_checked = button_checked(app.chk_aec);
                    app.auto_teams_checked = button_checked(app.chk_auto_teams);
                    app.auto_start_checked = button_checked(app.chk_auto_start);
                    unsafe {
                        KillTimer(app.hwnd, TIMER_WATCH);
                        // Destroy all child windows manually (avoid EnumChildWindows skip bug)
                        let mut child = GetWindow(app.hwnd, GW_CHILD);
                        while !child.is_null() {
                            let next = GetWindow(child, GW_HWNDNEXT);
                            DestroyWindow(child);
                            child = next;
                        }
                        app.combo = null_mut();
                        app.combo_mic = null_mut();
                        app.btn_start = null_mut();
                        app.btn_refresh = null_mut();
                        app.btn_open_folder = null_mut();
                        app.record_count_text = null_mut();
                        app.status = null_mut();
                        app.chk_aec = null_mut();
                        app.chk_auto_teams = null_mut();
                        app.recreate_controls(hwnd);
                        app.refresh_devices();
                        SetTimer(hwnd, TIMER_WATCH, 2000, null_mut());
                    }
                });
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }

        WM_TRAYICON => {
            if lp == WM_LBUTTONUP as LPARAM || lp == WM_LBUTTONDBLCLK as LPARAM {
                APP.with(|cell| cell.borrow_mut().restore_from_tray());
            } else if lp == WM_RBUTTONUP as LPARAM {
                // 这里不持有借用：show_tray_menu 内部会泵消息
                show_tray_menu(hwnd);
            }
            0
        }

        WM_TIMER => {
            if wp == TIMER_ID {
                APP.with(|cell| {
                    let mut app = cell.borrow_mut();
                    if app.session.is_some() {
                        app.show_recording_status();
                    }
                });
            } else if wp == TIMER_WATCH {
                // 自动模式下失败只写状态栏，不会弹窗
                APP.with(|cell| cell.borrow_mut().on_watch_tick());
            }
            0
        }

        WM_RECORD_ERROR => {
            // 收回采集线程投递过来的 Box<String>
            let message = *Box::from_raw(lp as *mut String);
            APP.with(|cell| {
                let mut app = cell.borrow_mut();
                if app.session.is_some() {
                    app.stop_recording();
                }
            });
            let ts = timestamp();
            PendingMessage::recording_error(message.clone()).show(hwnd);
            APP.with(|cell| cell.borrow_mut().set_status(&format!("结束: {ts}\r\n录制中断：{message}")));
            0
        }

        WM_CLOSE => {
            APP.with(|cell| {
                let mut app = cell.borrow_mut();
                if app.session.is_some() {
                    app.stop_recording();
                }
                app.remove_tray_icon();
            });
            DestroyWindow(hwnd);
            0
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 创建窗口并跑消息循环（等价于 C++ 版的 `wWinMain`）。
pub fn run() -> i32 {
    // Safety: 整个函数都在 UI 线程上执行；所有句柄都来自系统 API。
    unsafe {
        // 单进程限制
        let class_name = to_wide("DesktopAudioRecorder");
        let exist = FindWindowW(class_name.as_ptr(), null_mut());
        if !exist.is_null() {
            SetForegroundWindow(exist);
            ShowWindow(exist, SW_RESTORE);
            return 0;
        }

        // 让通用控件使用系统主题（主题清单由 build.rs 嵌入）
        load_config();
        let _com = ComApartment::sta();
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as DWORD,
            dwICC: ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let instance = GetModuleHandleW(null_mut());
        let class_name = to_wide("DesktopAudioRecorder");
        let window_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as UINT,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: LoadImageW(
                instance,
                make_int_resource(IDI_APP),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            ) as HICON,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW as usize as LPCWSTR) as HCURSOR,
            hbrBackground: (COLOR_BTNFACE + 1) as usize as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: LoadImageW(
                instance,
                make_int_resource(IDI_APP),
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_SHARED,
            ) as HICON,
        };
        RegisterClassExW(&window_class);

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rect = RECT { left: 0, top: 0, right: 520, bottom: 280 };
        AdjustWindowRect(&mut rect, style, FALSE);

        let title = to_wide(tr("UI", "app_title"));
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );
        if hwnd.is_null() {
            return 1;
        }

        // SW_SHOWDEFAULT ≈ wWinMain 收到的 nCmdShow
        ShowWindow(hwnd, SW_SHOWDEFAULT);
        UpdateWindow(hwnd);

        let mut message: MSG = core::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        0
    }
}
