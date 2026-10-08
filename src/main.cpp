// 桌面音频录制小工具（Windows）
// 同时录制：播放设备声音（WASAPI Loopback）+ 麦克风输入，混音后编码为 MP3
// 采集逻辑剥离自 OBS 的 win-wasapi 插件，不依赖 libobs。

#include <windows.h>
#include <windowsx.h>

#include <commctrl.h>
#include <mmdeviceapi.h>
#include <shellapi.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "audio_mixer.h"
#include "mp3_writer.h"
#include "resource.h"
#include "teams_watcher.h"
#include "wasapi_capture.h"

// 让通用控件使用系统主题
#pragma comment(linker,                                              \
		  "\"/manifestdependency:type='win32' "             \
		  "name='Microsoft.Windows.Common-Controls' "       \
		  "version='6.0.0.0' processorArchitecture='*' "    \
		  "publicKeyToken='6595b64144ccf1df' language='*'\"")

namespace {

// PKEY_Device_FriendlyName（文档公开的固定值），避免引入 functiondiscoverykeys_devpkey.h
const PROPERTYKEY kPKeyDeviceFriendlyName = {{0xa45c254e, 0xdf1c, 0x4efd,
					      {0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0}},
					     14};

constexpr int IDC_COMBO_DEVICE = 1001;
constexpr int IDC_BTN_START = 1002;
constexpr int IDC_BTN_REFRESH = 1003;
constexpr int IDC_EDIT_STATUS = 1004;
constexpr int IDC_COMBO_MIC = 1005;
constexpr int IDC_CHK_AEC = 1006;
constexpr int IDC_CHK_AUTOTEAMS = 1007;
constexpr UINT_PTR TIMER_ID = 1;
constexpr UINT_PTR TIMER_WATCH = 2;
constexpr UINT WM_RECORD_ERROR = WM_APP + 1;
constexpr UINT WM_TRAYICON = WM_APP + 2;
constexpr UINT TRAY_UID = 1;
constexpr int IDM_TRAY_OPEN = 2001;
constexpr int IDM_TRAY_EXIT = 2002;
constexpr uint32_t BITRATE = 192000; // 192 kbps
constexpr int WATCH_GRACE_POLLS = 5; // 会议结束防抖：连续 5 次未检测到才停录（2s×5=10s）

HWND g_hwnd{}, g_combo{}, g_comboMic{}, g_btnStart{}, g_btnRefresh{}, g_status{}, g_chkAec{}, g_chkAutoTeams{};
std::vector<std::wstring> g_renderIds; // 与播放下拉框索引一一对应
std::vector<std::wstring> g_micIds;    // 麦克风下拉框：索引 0 = 不录制，之后对应此数组

std::unique_ptr<WasapiCapture> g_capture; // 桌面（loopback）
std::unique_ptr<WasapiCapture> g_micCap;  // 麦克风
std::unique_ptr<AudioMixer> g_mixer;
std::unique_ptr<Mp3Writer> g_writer;
bool g_recording = false;
bool g_inTray = false;
bool g_autoStarted = false;  // 当前录制由 Teams 看门狗自动触发
bool g_autoSuppress = false; // 用户手动停止了自动录制：同一场会议内不再自动开始
int g_inactivePolls = 0;     // 会议结束防抖计数
ULONGLONG g_startTick = 0;
std::wstring g_outPath;
std::wstring g_deviceName;
std::wstring g_micName;

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
	std::wstring tip = L"桌面音频录制";
	if (g_recording)
		tip += L"（录制中）";
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
	AppendMenuW(menu, MF_STRING, IDM_TRAY_OPEN, L"打开");
	AppendMenuW(menu, MF_STRING, IDM_TRAY_EXIT, L"退出");
	POINT pt;
	GetCursorPos(&pt);
	SetForegroundWindow(g_hwnd); // 保证菜单点击别处能自动关闭
	TrackPopupMenu(menu, TPM_BOTTOMALIGN | TPM_LEFTALIGN, pt.x, pt.y, 0, g_hwnd, nullptr);
	DestroyMenu(menu);
}

void SetStatus(const std::wstring &text)
{
	SetWindowTextW(g_status, text.c_str());
}

std::wstring FormatDuration(ULONGLONG ms)
{
	const ULONGLONG s = ms / 1000;
	wchar_t buf[64];
	if (s >= 3600)
		swprintf_s(buf, L"%llu:%02llu:%02llu", s / 3600, (s / 60) % 60, s % 60);
	else
		swprintf_s(buf, L"%02llu:%02llu", s / 60, s % 60);
	return buf;
}

void ShowRecordingStatus()
{
	std::wstring text = L"录制中  已录 " + FormatDuration(GetTickCount64() - g_startTick);
	text += L"\r\n播放设备: " + g_deviceName;
	text += L"\r\n麦克风: " + g_micName;
	text += L"\r\n文件: " + g_outPath;
	SetStatus(text);
}

// 枚举音频端点填充下拉框；flow=eRender 播放设备，eCapture 录音设备
// addNone 时在索引 0 插入“（不录制）”项
void FillDeviceCombo(HWND combo, std::vector<std::wstring> &ids, EDataFlow flow, bool addNone)
{
	ComboBox_ResetContent(combo);
	ids.clear();

	int base = 0;
	if (addNone) {
		ComboBox_AddString(combo, L"（不录制）");
		base = 1;
	}

	IMMDeviceEnumerator *enumerator = nullptr;
	HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
				      __uuidof(IMMDeviceEnumerator), reinterpret_cast<void **>(&enumerator));
	if (FAILED(hr)) {
		SetStatus(L"初始化设备枚举器失败。");
		if (addNone)
			ComboBox_SetCurSel(combo, 0);
		return;
	}

	std::wstring defaultId;
	IMMDevice *defDev = nullptr;
	if (SUCCEEDED(enumerator->GetDefaultAudioEndpoint(flow, eConsole, &defDev))) {
		LPWSTR id = nullptr;
		if (SUCCEEDED(defDev->GetId(&id)) && id) {
			defaultId = id;
			CoTaskMemFree(id);
		}
		defDev->Release();
	}

	IMMDeviceCollection *collection = nullptr;
	int selectIdx = addNone ? 0 : -1;
	if (SUCCEEDED(enumerator->EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE, &collection))) {
		UINT count = 0;
		collection->GetCount(&count);
		for (UINT i = 0; i < count; ++i) {
			IMMDevice *dev = nullptr;
			if (FAILED(collection->Item(i, &dev)))
				continue;

			LPWSTR id = nullptr;
			dev->GetId(&id);

			std::wstring name;
			IPropertyStore *store = nullptr;
			if (SUCCEEDED(dev->OpenPropertyStore(STGM_READ, &store))) {
				PROPVARIANT pv;
				PropVariantInit(&pv);
				if (SUCCEEDED(store->GetValue(kPKeyDeviceFriendlyName, &pv)) &&
				    pv.vt == VT_LPWSTR && pv.pwszVal)
					name = pv.pwszVal;
				PropVariantClear(&pv);
				store->Release();
			}

			const int idx = ComboBox_AddString(combo, name.empty() ? L"(未知设备)" : name.c_str());
			ids.emplace_back(id ? id : L"");
			if (id && defaultId == id)
				selectIdx = idx;

			if (id)
				CoTaskMemFree(id);
			dev->Release();
		}
		collection->Release();
	}
	enumerator->Release();

	if (ids.empty() && !addNone) {
		// 播放设备一个都没有就没法录了
		SetStatus(flow == eRender ? L"未找到可用的播放设备。" : L"");
	} else if (selectIdx < 0) {
		selectIdx = base; // 无默认设备时选第一个
	}
	ComboBox_SetCurSel(combo, selectIdx);
}

void RefreshDevices()
{
	EnableWindow(g_btnStart, FALSE);
	FillDeviceCombo(g_combo, g_renderIds, eRender, false);
	FillDeviceCombo(g_comboMic, g_micIds, eCapture, true);
	if (!g_renderIds.empty())
		EnableWindow(g_btnStart, TRUE);
}

std::wstring MakeOutputPath()
{
	wchar_t exePath[MAX_PATH]{};
	GetModuleFileNameW(nullptr, exePath, MAX_PATH);
	SYSTEMTIME st{};
	GetLocalTime(&st);
	wchar_t name[128];
	swprintf_s(name, L"recording-%04d%02d%02d-%02d%02d%02d.mp3", st.wYear, st.wMonth, st.wDay, st.wHour,
		   st.wMinute, st.wSecond);
	return (std::filesystem::path(exePath).parent_path() / name).wstring();
}

bool StartRecording(bool fromAuto)
{
	// fromAuto=true 时失败只写状态栏不弹窗（看门狗场景避免反复弹窗）
	auto fail = [&](const std::wstring &msg) {
		if (fromAuto)
			SetStatus(msg);
		else
			MessageBoxW(g_hwnd, msg.c_str(), L"错误", MB_ICONERROR);
		return false;
	};

	const int sel = ComboBox_GetCurSel(g_combo);
	if (sel < 0 || static_cast<size_t>(sel) >= g_renderIds.size()) {
		if (!fromAuto)
			MessageBoxW(g_hwnd, L"请先选择一个播放设备。", L"提示", MB_ICONINFORMATION);
		return false;
	}
	const int micSel = ComboBox_GetCurSel(g_comboMic); // 0 = 不录制

	wchar_t nameBuf[256]{};
	GetWindowTextW(g_combo, nameBuf, 256);
	g_deviceName = nameBuf;
	g_micName = L"（不录制）";

	std::wstring err;

	// 1) 桌面 loopback 采集
	auto capture = std::make_unique<WasapiCapture>();
	if (!capture->Start(g_renderIds[sel], true, err))
		return fail(L"启动播放设备采集失败：\r\n" + err);

	// 2) 麦克风采集（可选）
	std::unique_ptr<WasapiCapture> micCap;
	if (micSel > 0 && static_cast<size_t>(micSel - 1) < g_micIds.size()) {
		micCap = std::make_unique<WasapiCapture>();
		if (!micCap->Start(g_micIds[micSel - 1], false, err)) {
			capture->Stop();
			return fail(L"启动麦克风采集失败：\r\n" + err + L"\r\n（可在麦克风下拉框选「不录制」）");
		}
		GetWindowTextW(g_comboMic, nameBuf, 256);
		g_micName = nameBuf;
	}

	// 3) 混音器 + 编码器（输出格式跟随桌面流）
	auto mixer = std::make_unique<AudioMixer>();
	auto writer = std::make_unique<Mp3Writer>();
	g_outPath = MakeOutputPath();
	if (!writer->Open(g_outPath, capture->SampleRate(), capture->Channels(), BITRATE, err)) {
		if (micCap)
			micCap->Stop();
		capture->Stop();
		return fail(L"创建 MP3 编码器失败：\r\n" + err + L"\r\n（Windows N 版本需安装 Media Feature Pack）");
	}

	Mp3Writer *w = writer.get();
	mixer->Open(capture->SampleRate(), capture->Channels(),
		    [w](const int16_t *d, uint32_t f) { w->Push(d, f); });
	mixer->SetEchoCancellation(Button_GetCheck(g_chkAec) == BST_CHECKED);
	if (micCap)
		mixer->SetMicFormat(micCap->SampleRate(), micCap->Channels());

	AudioMixer *mx = mixer.get();
	HWND hwnd = g_hwnd;
	auto errFn = [hwnd](const std::wstring &msg) {
		PostMessageW(hwnd, WM_RECORD_ERROR, 0, reinterpret_cast<LPARAM>(new std::wstring(msg)));
	};
	capture->Begin([mx](const int16_t *d, uint32_t f) { mx->PushDesktop(d, f); }, errFn);
	if (micCap)
		micCap->Begin([mx](const int16_t *d, uint32_t f) { mx->PushMic(d, f); }, errFn);

	g_capture = std::move(capture);
	g_micCap = std::move(micCap);
	g_mixer = std::move(mixer);
	g_writer = std::move(writer);
	g_recording = true;
	g_startTick = GetTickCount64();
	SetTimer(g_hwnd, TIMER_ID, 500, nullptr);

	SetWindowTextW(g_btnStart, L"停止录制");
	EnableWindow(g_combo, FALSE);
	EnableWindow(g_comboMic, FALSE);
	EnableWindow(g_chkAec, FALSE);
	EnableWindow(g_btnRefresh, FALSE);
	ShowRecordingStatus();
	return true;
}

void StopRecording()
{
	if (!g_recording)
		return;
	g_recording = false;
	KillTimer(g_hwnd, TIMER_ID);

	HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
	if (g_micCap)
		g_micCap->Stop();
	g_capture->Stop();
	g_writer->Close();
	SetCursor(old);

	std::wstring status;
	if (g_writer->HasError()) {
		status = L"录制中断：" + g_writer->Error();
		if (std::filesystem::exists(g_outPath))
			status += L"\r\n文件: " + g_outPath;
	} else {
		std::wstring size = L"?";
		std::error_code ec;
		const auto bytes = std::filesystem::file_size(g_outPath, ec);
		if (!ec) {
			wchar_t b[64];
			swprintf_s(b, L"%.1f MB", bytes / 1048576.0);
			size = b;
		}
		status = L"录制完成  时长 " + FormatDuration(GetTickCount64() - g_startTick) + L"  大小 " + size +
			 L"\r\n文件: " + g_outPath;
	}
	SetStatus(status);

	// 用户手动停止了自动开始的录制 → 本次会议内抑制自动重开
	if (g_autoStarted) {
		g_autoStarted = false;
		if (Button_GetCheck(g_chkAutoTeams) == BST_CHECKED && TeamsWatcher::MeetingActive())
			g_autoSuppress = true;
	}

	g_micCap.reset();
	g_capture.reset();
	g_mixer.reset();
	g_writer.reset();
	SetWindowTextW(g_btnStart, L"开始录制");
	EnableWindow(g_combo, TRUE);
	EnableWindow(g_comboMic, TRUE);
	EnableWindow(g_chkAec, TRUE);
	EnableWindow(g_btnRefresh, TRUE);
}

// Teams 会议看门狗：勾选后每 2s 检测一次。
// 检测到会议 → 自动开始录制；会议消失持续 10s（防抖）→ 自动停止。
void OnWatchTick()
{
	if (Button_GetCheck(g_chkAutoTeams) != BST_CHECKED)
		return;

	if (TeamsWatcher::MeetingActive()) {
		g_inactivePolls = 0;
		if (!g_recording && !g_autoSuppress) {
			if (StartRecording(true))
				g_autoStarted = true;
			else
				g_autoSuppress = true; // 自动开始失败：本次会议内不再反复尝试
		}
	} else {
		g_autoSuppress = false;
		if (g_recording && g_autoStarted) {
			if (++g_inactivePolls >= WATCH_GRACE_POLLS) {
				StopRecording();
				g_inactivePolls = 0;
				SetStatus(L"Teams 会议已结束，录制自动停止。\r\n文件: " + g_outPath);
			}
		} else {
			g_inactivePolls = 0;
		}
	}
}

LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
	switch (msg) {
	case WM_CREATE: {
		g_hwnd = hwnd;
		HFONT font = static_cast<HFONT>(GetStockObject(DEFAULT_GUI_FONT));
		auto make = [&](const wchar_t *cls, const wchar_t *text, DWORD style, int x, int y, int w, int h,
				int id) {
			HWND c = CreateWindowExW(0, cls, text, style | WS_CHILD | WS_VISIBLE, x, y, w, h, hwnd,
						 reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), nullptr, nullptr);
			SendMessageW(c, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
			return c;
		};

		make(L"STATIC", L"播放设备:", SS_LEFT, 12, 16, 64, 18, -1);
		g_combo = make(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_VSCROLL, 80, 12, 352, 300, IDC_COMBO_DEVICE);
		g_btnRefresh = make(L"BUTTON", L"刷新", 0, 440, 11, 68, 25, IDC_BTN_REFRESH);
		make(L"STATIC", L"麦克风:", SS_LEFT, 12, 50, 64, 18, -1);
		g_comboMic = make(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_VSCROLL, 80, 46, 352, 300, IDC_COMBO_MIC);
		g_btnStart = make(L"BUTTON", L"开始录制", 0, 12, 84, 130, 30, IDC_BTN_START);
		g_chkAec = make(L"BUTTON", L"回声消除 (AEC)", BS_AUTOCHECKBOX, 156, 88, 120, 22, IDC_CHK_AEC);
		Button_SetCheck(g_chkAec, BST_CHECKED);
		g_chkAutoTeams = make(L"BUTTON", L"Teams 会议自动录制", BS_AUTOCHECKBOX, 288, 88, 160, 22,
				      IDC_CHK_AUTOTEAMS);

		g_status = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"",
					   WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL |
						   WS_VSCROLL,
					   12, 124, 496, 114, hwnd,
					   reinterpret_cast<HMENU>(static_cast<INT_PTR>(IDC_EDIT_STATUS)),
					   nullptr, nullptr);
		SendMessageW(g_status, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);

		SetStatus(L"就绪。\r\n录制「播放设备」输出的声音（会议中对方的声音）并可同时混入「麦克风」"
			  L"（自己的声音），MP3 保存到程序所在目录。\r\n"
			  L"勾选「Teams 会议自动录制」后，检测到会议开始/结束会自动开始/停止录制。");
		RefreshDevices();
		SetTimer(hwnd, TIMER_WATCH, 2000, nullptr); // Teams 看门狗
		return 0;
	}

	case WM_COMMAND:
		switch (LOWORD(wp)) {
		case IDC_BTN_START:
			g_recording ? StopRecording() : (void)StartRecording(false);
			return 0;
		case IDC_BTN_REFRESH:
			if (!g_recording) {
				RefreshDevices();
				SetStatus(L"设备列表已刷新。");
			}
			return 0;
		case IDC_CHK_AUTOTEAMS:
			if (Button_GetCheck(g_chkAutoTeams) == BST_CHECKED) {
				g_autoSuppress = false;
				g_inactivePolls = 0;
				if (!g_recording)
					SetStatus(L"自动录制已开启：等待 Teams 会议开始…");
			}
			return 0;
		case IDM_TRAY_OPEN:
			RestoreFromTray();
			return 0;
		case IDM_TRAY_EXIT:
			SendMessageW(hwnd, WM_CLOSE, 0, 0);
			return 0;
		}
		break;

	case WM_SYSCOMMAND:
		if ((wp & 0xFFF0) == SC_MINIMIZE) {
			MinimizeToTray();
			return 0;
		}
		break;

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

	case WM_RECORD_ERROR: {
		std::unique_ptr<std::wstring> m(reinterpret_cast<std::wstring *>(lp));
		if (g_recording)
			StopRecording();
		MessageBoxW(hwnd, m->c_str(), L"录制错误", MB_ICONERROR);
		SetStatus(L"录制中断：" + *m);
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

} // namespace

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int showCmd)
{
	CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
	INITCOMMONCONTROLSEX icc{sizeof(icc), ICC_STANDARD_CLASSES};
	InitCommonControlsEx(&icc);

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

	RECT rc{0, 0, 520, 250};
	AdjustWindowRect(&rc, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, FALSE);
	HWND hwnd = CreateWindowExW(0, wc.lpszClassName, L"桌面音频录制 (Desktop Audio Recorder)",
				    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, CW_USEDEFAULT,
				    CW_USEDEFAULT, rc.right - rc.left, rc.bottom - rc.top, nullptr, nullptr,
				    instance, nullptr);

	ShowWindow(hwnd, showCmd);
	UpdateWindow(hwnd);

	MSG msg;
	while (GetMessageW(&msg, nullptr, 0, 0)) {
		TranslateMessage(&msg);
		DispatchMessageW(&msg);
	}

	CoUninitialize();
	return 0;
}
