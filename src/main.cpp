// 桌面音频录制小工具（Windows）
// 同时录制：播放设备声音（WASAPI Loopback）+ 麦克风输入，混音后编码为 MP3

#include <windows.h>
#include <windowsx.h>
#include <commctrl.h>
#include <mmdeviceapi.h>
#include <shellapi.h>
#include <filesystem>

#include "localization.h"
#include "logger.h"
#include "recorder.h"
#include "resource.h"

#pragma comment(linker,                                                                  \
    "\"/manifestdependency:type='win32' "                                                \
    "name='Microsoft.Windows.Common-Controls' "                                          \
    "version='6.0.0.0' processorArchitecture='*' "                                       \
    "publicKeyToken='6595b64144ccf1df' language='*'\"")

constexpr int IDC_COMBO_DEVICE = 1001;
constexpr int IDC_BTN_START = 1002;
constexpr int IDC_BTN_REFRESH = 1003;
constexpr int IDC_EDIT_STATUS = 1004;
constexpr int IDC_COMBO_MIC = 1005;
constexpr int IDC_CHK_AEC = 1006;
constexpr int IDC_CHK_AUTOTEAMS = 1007;
constexpr int IDC_BTN_OPENFOLDER = 1008;
constexpr UINT_PTR TIMER_ID = 1;
constexpr UINT_PTR TIMER_WATCH = 2;
constexpr UINT WM_RECORD_ERROR = WM_APP + 1;
constexpr UINT WM_TRAYICON = WM_APP + 2;
constexpr UINT TRAY_UID = 1;
constexpr int IDM_TRAY_OPEN = 2001;
constexpr int IDM_TRAY_EXIT = 2002;
constexpr int IDM_SYS_ABOUT = 3001;
constexpr int IDM_LANG_ZHCN = 3002;
constexpr int IDM_LANG_ENUS = 3003;

bool g_inTray = false;
static bool g_created = false;
static bool g_aecSaved = true;
static bool g_autoTeamsSaved = false;

// ---- 系统托盘 ----

void MinimizeToTray()
{
	NOTIFYICONDATAW nid{sizeof(nid)};
	nid.hWnd = g_hwnd;
	nid.uID = TRAY_UID;
	nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
	nid.uCallbackMessage = WM_TRAYICON;
	nid.hIcon = static_cast<HICON>(LoadImageW(GetModuleHandleW(nullptr), MAKEINTRESOURCEW(IDI_APP), IMAGE_ICON,
						  GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON),
						  LR_SHARED));
	std::wstring tip = S(L"UI", L"app_title");
	if (g_recording)
		tip += std::wstring(L" (") + S(L"Status", L"recording") + L")";
	wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
	Shell_NotifyIconW(NIM_ADD, &nid);

	ShowWindow(g_hwnd, SW_HIDE);
	g_inTray = true;
}

void RemoveTrayIcon()
{
	if (!g_inTray)
		return;
	NOTIFYICONDATAW nid{sizeof(nid)};
	nid.hWnd = g_hwnd;
	nid.uID = TRAY_UID;
	Shell_NotifyIconW(NIM_DELETE, &nid);
	g_inTray = false;
}

void RestoreFromTray()
{
	if (!g_inTray)
		return;
	RemoveTrayIcon();
	ShowWindow(g_hwnd, SW_SHOW);
	ShowWindow(g_hwnd, SW_RESTORE);
	SetForegroundWindow(g_hwnd);
}

void ShowTrayMenu()
{
	HMENU menu = CreatePopupMenu();
	AppendMenuW(menu, MF_STRING, IDM_TRAY_OPEN, S(L"Menu", L"tray_open"));
	AppendMenuW(menu, MF_STRING, IDM_TRAY_EXIT, S(L"Menu", L"tray_exit"));
	POINT pt;
	GetCursorPos(&pt);
	SetForegroundWindow(g_hwnd);
	TrackPopupMenu(menu, TPM_BOTTOMALIGN | TPM_LEFTALIGN, pt.x, pt.y, 0, g_hwnd, nullptr);
	DestroyMenu(menu);
}

// ---- UI 重建 ----

void RecreateControls(HWND hwnd)
{
	HFONT font = static_cast<HFONT>(GetStockObject(DEFAULT_GUI_FONT));
	auto make = [&](const wchar_t *cls, const wchar_t *text, DWORD style, int x, int y, int w, int h,
			int id)
			{
		HWND c = CreateWindowExW(0, cls, text, style | WS_CHILD | WS_VISIBLE, x, y, w, h, hwnd,
					 reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), nullptr, nullptr);
		SendMessageW(c, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
		return c;
	};

	make(L"STATIC", S(L"UI", L"label_playback"), SS_LEFT, 12, 16, 64, 18, -1);
	g_combo = make(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_VSCROLL, 80, 12, 352, 300, IDC_COMBO_DEVICE);
	g_btnRefresh = make(L"BUTTON", S(L"UI", L"btn_refresh"), 0, 440, 11, 68, 25, IDC_BTN_REFRESH);
	make(L"STATIC", S(L"UI", L"label_mic"), SS_LEFT, 12, 50, 64, 18, -1);
	g_comboMic = make(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_VSCROLL, 80, 46, 352, 300, IDC_COMBO_MIC);
	g_btnStart = make(L"BUTTON", S(L"UI", L"btn_start"), 0, 12, 84, 130, 25, IDC_BTN_START);
	g_recordCountText = make(L"STATIC", L"", SS_LEFT, 146, 88, 70, 18, -1);
	wchar_t countBuf[32]{};
	swprintf_s(countBuf, L"%s: %d", S(L"UI", L"record_count"), g_recordCount);
	SetWindowTextW(g_recordCountText, countBuf);
	g_btnOpenFolder = make(L"BUTTON", S(L"UI", L"btn_open_folder"), 0, 220, 84, 100, 25, IDC_BTN_OPENFOLDER);
	g_chkAec = make(L"BUTTON", S(L"UI", L"chk_aec"), BS_AUTOCHECKBOX, 12, 118, 160, 22, IDC_CHK_AEC);
	Button_SetCheck(g_chkAec, g_aecSaved ? BST_CHECKED : BST_UNCHECKED);
	g_chkAutoTeams = make(L"BUTTON", S(L"UI", L"chk_auto_teams"), BS_AUTOCHECKBOX, 180, 118, 220, 22,
			      IDC_CHK_AUTOTEAMS);
	Button_SetCheck(g_chkAutoTeams, g_autoTeamsSaved ? BST_CHECKED : BST_UNCHECKED);

	g_status = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"",
				   WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL |
					   WS_VSCROLL,
				   12, 148, 496, 110, hwnd,
				   reinterpret_cast<HMENU>(static_cast<INT_PTR>(IDC_EDIT_STATUS)),
				   nullptr, nullptr);
	SendMessageW(g_status, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);

	SetStatus(std::wstring(S(L"Status", L"ready")) + L"\r\n" +
		  S(L"Status", L"initial_help") + L"\r\n" +
		  S(L"Status", L"legal_warning"));

	HMENU sysMenu = GetSystemMenu(hwnd, FALSE);
	int nCount = GetMenuItemCount(sysMenu);
	while (nCount > 7)
	{
		RemoveMenu(sysMenu, nCount - 1, MF_BYPOSITION);
		--nCount;
	}
	AppendMenuW(sysMenu, MF_SEPARATOR, 0, nullptr);
	AppendMenuW(sysMenu, MF_STRING, IDM_SYS_ABOUT, S(L"Menu", L"sys_about"));

	HMENU langMenu = CreatePopupMenu();
	AppendMenuW(langMenu, MF_STRING | (g_lang == L"zh-CN" ? MF_CHECKED : 0), IDM_LANG_ZHCN, L"中文 (简体)");
	AppendMenuW(langMenu, MF_STRING | (g_lang == L"en-US" ? MF_CHECKED : 0), IDM_LANG_ENUS, L"English");
	AppendMenuW(sysMenu, MF_POPUP, reinterpret_cast<UINT_PTR>(langMenu), S(L"Menu", L"sys_language"));

	if (!g_created)
	{
		RefreshDevices();
		SetTimer(hwnd, TIMER_WATCH, 2000, nullptr);
		g_created = true;
	}
	else
	{
		FillDeviceCombo(g_combo, g_renderIds, eRender, false);
		FillDeviceCombo(g_comboMic, g_micIds, eCapture, true);
		if (g_recording)
		{
			SetWindowTextW(g_btnStart, S(L"UI", L"btn_stop"));
			EnableWindow(g_combo, FALSE);
			EnableWindow(g_comboMic, FALSE);
		}
		SetTimer(hwnd, TIMER_WATCH, 2000, nullptr);
	}
}

void ReloadUI()
{
	HWND hwnd = g_hwnd;
	if (!hwnd) return;

	g_aecSaved = (g_chkAec && Button_GetCheck(g_chkAec) == BST_CHECKED);
	g_autoTeamsSaved = (g_chkAutoTeams && Button_GetCheck(g_chkAutoTeams) == BST_CHECKED);

	KillTimer(hwnd, TIMER_WATCH);

	HWND child = GetWindow(hwnd, GW_CHILD);
	while (child)
	{
		HWND next = GetWindow(child, GW_HWNDNEXT);
		DestroyWindow(child);
		child = next;
	}
	g_combo = g_comboMic = g_btnStart = g_btnRefresh = g_btnOpenFolder = g_status = g_chkAec = g_chkAutoTeams = g_recordCountText = nullptr;
	RecreateControls(hwnd);
}

// ---- 窗口过程 ----

LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
	switch (msg)
	{
	case WM_CREATE:
		g_hwnd = hwnd;
		g_created = false;
		RecreateControls(hwnd);
		return 0;

	case WM_COMMAND:
		switch (LOWORD(wp))
		{
		case IDC_BTN_START:
			g_recording ? StopRecording() : (void)StartRecording(false);
			return 0;
		case IDC_BTN_REFRESH:
			if (!g_recording)
			{
				RefreshDevices();
				SetStatus(S(L"Status", L"devices_refreshed"));
			}
			return 0;
		case IDC_CHK_AUTOTEAMS: {
			bool checked = Button_GetCheck(g_chkAutoTeams) == BST_CHECKED;
			g_autoTeamsSaved = checked;
			if (checked)
			{
				g_autoSuppress = false;
				g_inactivePolls = 0;
				AppendLog(L"Meeting monitor ON");
				if (!g_recording)
					SetStatus(S(L"Status", L"auto_teams_on"));
			}
			else
			{
				AppendLog(L"Meeting monitor OFF");
			}
			}
			return 0;
		case IDC_BTN_OPENFOLDER:
		{
			wchar_t exePath[MAX_PATH]{};
			GetModuleFileNameW(nullptr, exePath, MAX_PATH);
			std::filesystem::path dir = std::filesystem::path(exePath).parent_path();
			ShellExecuteW(hwnd, L"open", dir.c_str(), nullptr, nullptr, SW_SHOW);
			return 0;
		}
		case IDM_TRAY_OPEN:
			RestoreFromTray();
			return 0;
		case IDM_TRAY_EXIT:
			SendMessageW(hwnd, WM_CLOSE, 0, 0);
			return 0;
		}
		break;

	case WM_SYSCOMMAND:
	{
		const UINT cmd = wp & 0xFFF0;
		if (cmd == SC_MINIMIZE)
		{
			MinimizeToTray();
			return 0;
		}
		const UINT id = LOWORD(wp);
		if (id == IDM_SYS_ABOUT)
		{
			std::wstring about = std::wstring(S(L"About", L"line1")) + L"\r\n\r\n" +
					    S(L"About", L"line2") + L"\r\n" +
					    S(L"About", L"line3") + L"\r\n\r\n" +
					    S(L"About", L"line4") + L"\r\n" +
					    S(L"About", L"line5") + L"\r\n" +
					    S(L"About", L"line6") + L"\r\n" +
					    S(L"About", L"line7");
			MessageBoxW(hwnd, about.c_str(), S(L"About", L"title"), MB_OK | MB_ICONINFORMATION);
			return 0;
		}
		if (id == IDM_LANG_ZHCN || id == IDM_LANG_ENUS)
		{
			g_lang = (id == IDM_LANG_ZHCN) ? L"zh-CN" : L"en-US";
			SaveConfig();
			LoadLangFile(g_lang);
			ReloadUI();
			return 0;
		}
		break;
	}

	case WM_TRAYICON:
		if (lp == WM_LBUTTONUP || lp == WM_LBUTTONDBLCLK)
			RestoreFromTray();
		else if (lp == WM_RBUTTONUP)
			ShowTrayMenu();
		return 0;

	case WM_TIMER:
		if (wp == TIMER_ID && g_recording)
			ShowRecordingStatus();
		else if (wp == TIMER_WATCH)
			OnWatchTick();
		return 0;

	case WM_RECORD_ERROR:
	{
		std::unique_ptr<std::wstring> m(reinterpret_cast<std::wstring *>(lp));
		if (g_recording)
			StopRecording();
		MessageBoxW(hwnd, m->c_str(), S(L"About", L"title"), MB_ICONERROR);
		SetStatus(std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"end") + L": " + Timestamp() + L"\r\n" + S(L"Status", L"interrupted") + L"：" + *m);
		return 0;
	}

	case WM_CLOSE:
		if (g_recording)
			StopRecording();
		RemoveTrayIcon();
		DestroyWindow(hwnd);
		return 0;

	case WM_DESTROY:
		PostQuitMessage(0);
		return 0;
	}
	return DefWindowProcW(hwnd, msg, wp, lp);
}

// ---- 入口 ----

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int showCmd)
{
	// 单进程限制：查找是否已有同窗口类名的窗口
	HWND exist = FindWindowW(L"DesktopAudioRecorder", nullptr);
	if (exist)
	{
		SetForegroundWindow(exist);
		ShowWindow(exist, SW_RESTORE);
		return 0;
	}

	CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
	INITCOMMONCONTROLSEX icc{sizeof(icc), ICC_STANDARD_CLASSES};
	InitCommonControlsEx(&icc);

	LoadConfig();

	WNDCLASSEXW wc{sizeof(wc)};
	wc.lpfnWndProc = WndProc;
	wc.hInstance = instance;
	wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
	wc.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_BTNFACE + 1);
	wc.lpszClassName = L"DesktopAudioRecorder";
	wc.hIcon = static_cast<HICON>(
		LoadImageW(instance, MAKEINTRESOURCEW(IDI_APP), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE | LR_SHARED));
	wc.hIconSm = static_cast<HICON>(LoadImageW(instance, MAKEINTRESOURCEW(IDI_APP), IMAGE_ICON,
						   GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON),
						   LR_SHARED));
	RegisterClassExW(&wc);

	RECT rc{0, 0, 520, 280};
	AdjustWindowRect(&rc, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, FALSE);
	HWND hwnd = CreateWindowExW(0, wc.lpszClassName, S(L"UI", L"app_title"),
				    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, CW_USEDEFAULT,
				    CW_USEDEFAULT, rc.right - rc.left, rc.bottom - rc.top, nullptr, nullptr,
				    instance, nullptr);

	ShowWindow(hwnd, showCmd);
	UpdateWindow(hwnd);

	MSG msg;
	while (GetMessageW(&msg, nullptr, 0, 0))
	{
		TranslateMessage(&msg);
		DispatchMessageW(&msg);
	}

	CoUninitialize();
	return 0;
}
