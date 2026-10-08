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
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use core::ptr::{null, null_mut};

use crate::audio_mixer::AudioMixer;
use crate::ffi::system::{
    AdjustWindowRect, AppendMenuW, CoCreateInstance, CoTaskMemFree, ComApartment, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EnableWindow,
    GetCursorPos, GetLocalTime, GetMessageW, GetModuleFileNameW, GetModuleHandleW, GetStockObject,
    GetSystemMetrics, GetTickCount64, GetWindowTextW, InitCommonControlsEx, KillTimer, LoadCursorW,
    LoadImageW, MessageBoxW, PostMessageW, PostQuitMessage, PropVariantClear, RegisterClassExW,
    SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowTextW, Shell_NotifyIconW,
    ShowWindow, TrackPopupMenu, TranslateMessage, UpdateWindow, BM_GETCHECK, BM_SETCHECK,
    BS_AUTOCHECKBOX, BST_CHECKED, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL,
    CBS_DROPDOWNLIST, COLOR_BTNFACE, CW_USEDEFAULT, DEFAULT_GUI_FONT, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, ICC_STANDARD_CLASSES, IDC_ARROW, IDC_WAIT, IMAGE_ICON, INITCOMMONCONTROLSEX,
    LR_DEFAULTSIZE, LR_SHARED, MAX_PATH, MB_ICONERROR, MB_ICONINFORMATION, MF_STRING, MSG,
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, POINT, RECT, SC_MASK,
    SC_MINIMIZE, SM_CXSMICON, SM_CYSMICON, SS_LEFT, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWDEFAULT,
    SYSTEMTIME, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_SETFONT, WM_SYSCOMMAND,
    WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_VISIBLE, WS_VSCROLL,
};
use crate::ffi::wasapi::{
    eCapture, eConsole, eRender, EDataFlow, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator,
    IPropertyStore, CLSID_MMDeviceEnumerator, DEVICE_STATE_ACTIVE, PKEY_DEVICE_FRIENDLY_NAME,
    STGM_READ,
};
use crate::ffi::{
    failed, make_int_resource, succeeded, Com, ComPtr, DWORD, HBRUSH, HCURSOR, HICON, HMENU, HWND,
    LPARAM, LPCWSTR, LPWSTR, LRESULT, PROPVARIANT, SendableHwnd, UINT, WPARAM, CLSCTX_ALL, FALSE,
    TRUE, VT_LPWSTR,
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

const TIMER_ID: usize = 1;
const TIMER_WATCH: usize = 2;
const WM_RECORD_ERROR: UINT = WM_APP + 1;
const WM_TRAYICON: UINT = WM_APP + 2;
const TRAY_UID: UINT = 1;
const IDM_TRAY_OPEN: usize = 2001;
const IDM_TRAY_EXIT: usize = 2002;

/// 192 kbps
const BITRATE: u32 = 192_000;
/// 会议结束防抖：连续 5 次未检测到才停录（2s × 5 = 10s）
const WATCH_GRACE_POLLS: u32 = 5;
/// `resource.h` 里的 `IDI_APP`
const IDI_APP: u16 = 101;

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
        Self { text, title: "错误".to_string(), flags: MB_ICONERROR }
    }

    fn recording_error(text: String) -> Self {
        Self { text, title: "录制错误".to_string(), flags: MB_ICONERROR }
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
    status: HWND,
    chk_aec: HWND,
    chk_auto_teams: HWND,

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
            status: null_mut(),
            chk_aec: null_mut(),
            chk_auto_teams: null_mut(),
            render_ids: Vec::new(),
            mic_ids: Vec::new(),
            session: None,
            in_tray: false,
            auto_started: false,
            auto_suppress: false,
            inactive_polls: 0,
            start_tick: 0,
            out_path: PathBuf::new(),
            device_name: String::new(),
            mic_name: String::new(),
        }
    }

    // ---- 控件 ----

    fn set_status(&mut self, text: &str) {
        set_window_text(self.status, text);
    }

    fn on_create(&mut self, hwnd: HWND) {
        unsafe {
            self.hwnd = hwnd;
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

            make("STATIC", "播放设备:", SS_LEFT, 12, 16, 64, 18, -1);
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
            self.btn_refresh = make("BUTTON", "刷新", 0, 440, 11, 68, 25, IDC_BTN_REFRESH);
            make("STATIC", "麦克风:", SS_LEFT, 12, 50, 64, 18, -1);
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
            self.btn_start = make("BUTTON", "开始录制", 0, 12, 84, 130, 30, IDC_BTN_START);
            self.chk_aec = make(
                "BUTTON",
                "回声消除 (AEC)",
                BS_AUTOCHECKBOX,
                156,
                88,
                120,
                22,
                IDC_CHK_AEC,
            );
            button_set_check(self.chk_aec, true);
            self.chk_auto_teams = make(
                "BUTTON",
                "Teams 会议自动录制",
                BS_AUTOCHECKBOX,
                288,
                88,
                160,
                22,
                IDC_CHK_AUTOTEAMS,
            );

            let edit_class = to_wide("EDIT");
            let empty = to_wide("");
            self.status = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                edit_class.as_ptr(),
                empty.as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL,
                12,
                124,
                496,
                114,
                hwnd,
                IDC_EDIT_STATUS as usize as HMENU,
                null_mut(),
                null_mut(),
            );
            SendMessageW(self.status, WM_SETFONT, font as WPARAM, TRUE as LPARAM);

            self.set_status(
                "就绪。\r\n录制「播放设备」输出的声音（会议中对方的声音）并可同时混入「麦克风」\
                 （自己的声音），MP3 保存到程序所在目录。\r\n\
                 勾选「Teams 会议自动录制」后，检测到会议开始/结束会自动开始/停止录制。",
            );
            self.refresh_devices();
            // Teams 看门狗
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
            if self.session.is_none() {
                self.set_status("自动录制已开启：等待 Teams 会议开始…");
            }
        }
    }

    // ---- 录制 ----

    fn show_recording_status(&mut self) {
        let elapsed = unsafe { GetTickCount64() } - self.start_tick;
        let text = format!(
            "录制中  已录 {}\r\n播放设备: {}\r\n麦克风: {}\r\n文件: {}",
            format_duration(elapsed),
            self.device_name,
            self.mic_name,
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
                    Some(PendingMessage::info("请先选择一个播放设备。".to_string(), "提示"))
                },
            };
        }
        // 0 = 不录制
        let mic_sel = combo_cur_sel(self.combo_mic);

        self.device_name = combo_text(self.combo);
        self.mic_name = "（不录制）".to_string();

        // 1) 桌面 loopback 采集
        let device_id = self.render_ids[sel as usize].clone();
        let mut desktop = WasapiCapture::new();
        if let Err(e) = desktop.start(Some(&device_id), true) {
            return self.start_failure(from_auto, format!("启动播放设备采集失败：\r\n{e}"));
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
                    format!("启动麦克风采集失败：\r\n{e}\r\n（可在麦克风下拉框选「不录制」）"),
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
                    format!(
                        "创建 MP3 编码器失败：\r\n{e}\r\n（Windows N 版本需安装 Media Feature Pack）"
                    ),
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
        unsafe { SetTimer(self.hwnd, TIMER_ID, 500, null_mut()) };

        set_window_text(self.btn_start, "停止录制");
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

        let status = if has_error {
            let mut text = format!("录制中断：{error}");
            if self.out_path.exists() {
                text.push_str(&format!("\r\n文件: {}", self.out_path.display()));
            }
            text
        } else {
            let size = std::fs::metadata(&self.out_path)
                .map(|m| format!("{:.1} MB", m.len() as f64 / 1_048_576.0))
                .unwrap_or_else(|_| "?".to_string());
            let elapsed = unsafe { GetTickCount64() } - self.start_tick;
            format!(
                "录制完成  时长 {}  大小 {}\r\n文件: {}",
                format_duration(elapsed),
                size,
                self.out_path.display()
            )
        };
        self.set_status(&status);

        // 用户手动停止了自动开始的录制 → 本次会议内抑制自动重开
        if self.auto_started {
            self.auto_started = false;
            if button_checked(self.chk_auto_teams) && TeamsWatcher::meeting_active() {
                self.auto_suppress = true;
            }
        }

        set_window_text(self.btn_start, "开始录制");
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
                    self.stop_recording();
                    self.inactive_polls = 0;
                    let path = self.out_path.display().to_string();
                    self.set_status(&format!("Teams 会议已结束，录制自动停止。\r\n文件: {path}"));
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
            let mut tip = "桌面音频录制".to_string();
            if self.session.is_some() {
                tip.push_str("（录制中）");
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
        combo_add_string(combo, "（不录制）");
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
        return (ids, Some("初始化设备枚举器失败。".to_string()));
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

            let label = if name.is_empty() { "(未知设备)" } else { name.as_str() };
            let index = combo_add_string(combo, label);
            if !id.is_empty() && id == default_id {
                select_index = index;
            }
            ids.push(id);
        }
    }

    let status = if ids.is_empty() && !add_none {
        // 播放设备一个都没有就没法录了
        if flow == eRender {
            Some("未找到可用的播放设备。".to_string())
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
    let open = to_wide("打开");
    let exit = to_wide("退出");
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
                            app.set_status("设备列表已刷新。");
                        }
                        None
                    }
                    IDC_CHK_AUTOTEAMS => {
                        app.on_auto_teams_toggled();
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
            if (wp & SC_MASK) == SC_MINIMIZE {
                APP.with(|cell| cell.borrow_mut().minimize_to_tray());
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
            PendingMessage::recording_error(message.clone()).show(hwnd);
            APP.with(|cell| cell.borrow_mut().set_status(&format!("录制中断：{message}")));
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
        // 让通用控件使用系统主题（主题清单由 build.rs 嵌入）
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
        let mut rect = RECT { left: 0, top: 0, right: 520, bottom: 250 };
        AdjustWindowRect(&mut rect, style, FALSE);

        let title = to_wide("桌面音频录制 (Desktop Audio Recorder)");
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
