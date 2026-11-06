#include "recorder.h"
#include "localization.h"
#include "logger.h"
#include "teams_watcher.h"

#include <windowsx.h>
#include <commctrl.h>
#include <filesystem>
#include <memory>
#include <mutex>
#include <new>
#include <string>
#include <vector>

// PKEY_Device_FriendlyName
namespace
{
const PROPERTYKEY kPKeyDeviceFriendlyName = {{0xa45c254e, 0xdf1c, 0x4efd,
					      {0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0}},
					     14};
}

constexpr uint32_t BITRATE = 192000;
constexpr int WATCH_GRACE_POLLS = 5;
// 自动开始失败后的重试次数与退避（单位 = 看门狗 tick，即 2s）
constexpr int MAX_AUTO_START_RETRIES = 3;
constexpr int kRetryTicks[MAX_AUTO_START_RETRIES] = {1, 2, 4};

// 自动分割的可选时长（分钟）与对应的语言文件 key，顺序即下拉框顺序
constexpr int kSplitMinutes[4] = {15, 30, 60, 90};
constexpr const wchar_t *kSplitKeys[4] = {L"split_15", L"split_30", L"split_60", L"split_90"};

HWND g_hwnd{}, g_combo{}, g_comboMic{}, g_btnStart{}, g_btnRefresh{}, g_btnOpenFolder{}, g_status{}, g_chkAec{}, g_chkAutoTeams{}, g_recordCountText{}, g_chkAutoStart{}, g_chkAutoSplit{}, g_comboSplit{};
int g_recordCount = 0;
std::vector<std::wstring> g_renderIds;
std::vector<std::wstring> g_micIds;

std::unique_ptr<WasapiCapture> g_capture;
std::unique_ptr<WasapiCapture> g_micCap;
std::unique_ptr<AudioMixer> g_mixer;
std::unique_ptr<Mp3Writer> g_writer;
bool g_recording = false;
bool g_autoStarted = false;
bool g_autoSuppress = false;
int g_inactivePolls = 0;
ULONGLONG g_startTick = 0;
std::wstring g_startTime;
std::wstring g_outPath;
std::wstring g_deviceName;
std::wstring g_micName;
std::wstring g_meetingApp;

// 当前生效的编码器：采集线程经 g_wmtx 取用，切换分段文件时原子替换。
std::mutex g_wmtx;

// 自动分割状态（仅 UI 线程访问）
int g_splitMinutes = 0;
std::wstring g_splitBase;
int g_splitIndex = 0;
ULONGLONG g_segStartTick = 0;
uint32_t g_capRate = 0;
uint16_t g_capChannels = 0;
// 采集线程上报的运行期错误（编码器本身可能没出错），StopRecording 据此写“中断”日志
std::wstring g_pendingError;
// 本次录制使用的麦克风设备 id（重连时复用）
std::wstring g_micId;
// 自动开始的失败重试状态（看门狗 tick 计数）
int g_startRetries = 0;
int g_retryCountdown = 0;

void NoteRecordError(const std::wstring &msg)
{
	g_pendingError = msg;
}

void ResetAutoWatchState()
{
	g_autoSuppress = false;
	g_inactivePolls = 0;
	g_startRetries = 0;
	g_retryCountdown = 0;
}

void FillSplitCombo()
{
	ComboBox_ResetContent(g_comboSplit);
	for (const wchar_t *key : kSplitKeys)
		ComboBox_AddString(g_comboSplit, S(L"UI", key));
	int idx = 2;
	for (int i = 0; i < 4; ++i)
	{
		if (kSplitMinutes[i] == g_splitMinutesSaved)
		{
			idx = i;
			break;
		}
	}
	ComboBox_SetCurSel(g_comboSplit, idx);
}

// 把「自动分割录音文件」复选框 + 时长下拉框的当前状态同步到录制逻辑（每次开始录制都会调用）
void SyncSplitFromControls()
{
	g_autoSplitSaved = (g_chkAutoSplit && Button_GetCheck(g_chkAutoSplit) == BST_CHECKED);
	const int sel = ComboBox_GetCurSel(g_comboSplit);
	g_splitMinutesSaved = (sel >= 0 && sel < 4) ? kSplitMinutes[sel] : 60;
	g_splitMinutes = g_autoSplitSaved ? g_splitMinutesSaved : 0;
}

std::wstring MakeOutputBase()
{
	wchar_t exePath[MAX_PATH]{};
	GetModuleFileNameW(nullptr, exePath, MAX_PATH);
	SYSTEMTIME st{};
	GetLocalTime(&st);

	std::filesystem::path dir = std::filesystem::path(exePath).parent_path();
	if (!g_meetingApp.empty())
	{
		dir = dir / g_meetingApp;
		std::filesystem::create_directories(dir);
	}

	wchar_t name[256];
	swprintf_s(name, L"%s-%04d%02d%02d-%02d%02d%02d", g_meetingApp.empty() ? L"recording" : g_meetingApp.c_str(),
		   st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
	return (dir / name).wstring();
}

void SetStatus(const std::wstring &text)
{
	SetWindowTextW(g_status, text.c_str());
}

// 到达分割时限时切换输出文件：先打开新文件，成功后原子换掉采集线程正在写的编码器，
// 再冲刷并关闭旧文件。采集线程不受影响（切换期间最多丢一个 10ms 缓冲块）。
void MaybeSplitOutput()
{
	if (!g_recording || g_splitMinutes <= 0 || !g_writer)
		return;
	if (GetTickCount64() - g_segStartTick < static_cast<ULONGLONG>(g_splitMinutes) * 60000ULL)
		return;

	const std::wstring newPath = g_splitBase + L"_" + std::to_wstring(g_splitIndex + 1) + L".mp3";
	std::wstring err;
	auto next = std::make_unique<Mp3Writer>();
	if (!next->Open(newPath, g_capRate, g_capChannels, BITRATE, err))
	{
		AppendLog(L"split failed, splitting disabled for this recording: " + err);
		g_splitMinutes = 0;
		return;
	}

	std::unique_ptr<Mp3Writer> prev;
	{
		std::lock_guard<std::mutex> lk(g_wmtx);
		prev.swap(g_writer);
		g_writer = std::move(next);
	}
	if (prev)
	{
		if (prev->HasError())
			AppendLog(L"segment interrupted: " + prev->Error() + L"  file: " + g_outPath);
		prev->Close();
	}

	++g_splitIndex;
	g_outPath = newPath;
	g_segStartTick = GetTickCount64();
	AppendLog(L"split to next segment  file: " + g_outPath);
}

void FillDeviceCombo(HWND combo, std::vector<std::wstring> &ids, EDataFlow flow, bool addNone)
{
	ComboBox_ResetContent(combo);
	ids.clear();

	int base = 0;
	if (addNone)
	{
		ComboBox_AddString(combo, S(L"UI", L"mic_none"));
		base = 1;
	}

	IMMDeviceEnumerator *enumerator = nullptr;
	HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
				      __uuidof(IMMDeviceEnumerator), reinterpret_cast<void **>(&enumerator));
	if (FAILED(hr))
	{
		SetStatus(L"Init failed");
		if (addNone)
			ComboBox_SetCurSel(combo, 0);
		return;
	}

	std::wstring defaultId;
	IMMDevice *defDev = nullptr;
	if (SUCCEEDED(enumerator->GetDefaultAudioEndpoint(flow, eConsole, &defDev)))
	{
		LPWSTR id = nullptr;
		if (SUCCEEDED(defDev->GetId(&id)) && id)
		{
			defaultId = id;
			CoTaskMemFree(id);
		}
		defDev->Release();
	}

	IMMDeviceCollection *collection = nullptr;
	int selectIdx = addNone ? 0 : -1;
	if (SUCCEEDED(enumerator->EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE, &collection)))
	{
		UINT count = 0;
		collection->GetCount(&count);
		for (UINT i = 0; i < count; ++i)
		{
			IMMDevice *dev = nullptr;
			if (FAILED(collection->Item(i, &dev)))
				continue;

			LPWSTR id = nullptr;
			dev->GetId(&id);

			std::wstring name;
			IPropertyStore *store = nullptr;
			if (SUCCEEDED(dev->OpenPropertyStore(STGM_READ, &store)))
			{
				PROPVARIANT pv;
				PropVariantInit(&pv);
				if (SUCCEEDED(store->GetValue(kPKeyDeviceFriendlyName, &pv)) &&
				    pv.vt == VT_LPWSTR && pv.pwszVal)
					name = pv.pwszVal;
				PropVariantClear(&pv);
				store->Release();
			}

			const int idx = ComboBox_AddString(combo, name.empty() ? L"(?)" : name.c_str());
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

	if (ids.empty() && !addNone)
	{
		SetStatus(flow == eRender ? S(L"Status", L"error_no_playback") : L"");
	}
	else if (selectIdx < 0)
	{
		selectIdx = base;
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

int FindRenderIndex(const std::wstring &deviceId)
{
	if (deviceId.empty())
		return -1;
	for (size_t i = 0; i < g_renderIds.size(); ++i)
	{
		if (g_renderIds[i] == deviceId)
			return static_cast<int>(i);
	}
	return -1;
}

// 刷新设备列表，但把用户当前的选择尽量保留下来（FillDeviceCombo 会重置为默认设备）
void RefreshDevicesKeepingSelection()
{
	const bool wasRecording = g_recording;

	std::wstring keep, keepMic;
	const int sel = ComboBox_GetCurSel(g_combo);
	if (sel >= 0 && static_cast<size_t>(sel) < g_renderIds.size())
		keep = g_renderIds[sel];
	const int micSel = ComboBox_GetCurSel(g_comboMic);
	if (micSel > 0 && static_cast<size_t>(micSel - 1) < g_micIds.size())
		keepMic = g_micIds[micSel - 1];

	RefreshDevices();

	if (!keep.empty())
	{
		const int idx = FindRenderIndex(keep);
		if (idx >= 0)
			ComboBox_SetCurSel(g_combo, idx);
	}
	if (!keepMic.empty())
	{
		for (size_t i = 0; i < g_micIds.size(); ++i)
		{
			if (g_micIds[i] == keepMic)
			{
				ComboBox_SetCurSel(g_comboMic, static_cast<int>(i) + 1);
				break;
			}
		}
	}

	if (wasRecording)
	{
		EnableWindow(g_combo, FALSE);
		EnableWindow(g_comboMic, FALSE);
		EnableWindow(g_chkAec, FALSE);
		EnableWindow(g_btnRefresh, FALSE);
		EnableWindow(g_chkAutoSplit, FALSE);
		EnableWindow(g_comboSplit, FALSE);
	}
}

void ShowRecordingStatus()
{
	std::wstring text = std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"recording") + L"  " + S(L"Status", L"duration") + L" " + FormatDuration(GetTickCount64() - g_startTick);
	text += L"\r\n" + std::wstring(S(L"UI", L"label_playback")) + L" " + g_deviceName;
	text += L"\r\n" + std::wstring(S(L"UI", L"label_mic")) + L" " + g_micName;
	text += L"\r\n" + std::wstring(S(L"Status", L"file")) + L": " + g_outPath;
	SetStatus(text);
}

bool StartRecording(bool fromAuto, int renderOverride, const std::wstring &nameOverride)
{
	SyncSplitFromControls();
	g_pendingError.clear();
	g_startRetries = 0;
	g_retryCountdown = 0;
	if (!fromAuto)
		g_meetingApp.clear();

	auto fail = [&](const std::wstring &msg)
	{
		AppendLogLine(L"start failed:  " + msg);
		if (fromAuto)
			SetStatus(msg);
		else
			MessageBoxW(g_hwnd, msg.c_str(), S(L"About", L"title"), MB_ICONERROR);
		return false;
	};

	int sel = renderOverride;
	if (sel < 0 || static_cast<size_t>(sel) >= g_renderIds.size())
		sel = ComboBox_GetCurSel(g_combo);
	if (sel < 0 || static_cast<size_t>(sel) >= g_renderIds.size())
	{
		AppendLogLine(L"start failed:  " + std::wstring(S(L"Status", L"error_no_device")));
		if (!fromAuto)
			MessageBoxW(g_hwnd, S(L"Status", L"error_no_device"), S(L"UI", L"app_title"), MB_ICONINFORMATION);
		return false;
	}
	const int micSel = ComboBox_GetCurSel(g_comboMic);

	wchar_t nameBuf[256]{};
	g_deviceName = nameOverride;
	if (g_deviceName.empty())
	{
		GetWindowTextW(g_combo, nameBuf, 256);
		g_deviceName = nameBuf;
	}
	g_micName = S(L"UI", L"mic_none");
	g_micId.clear();

	std::wstring warn; // 麦克风失败但录制可以继续时的提示
	std::wstring err;

	auto capture = std::make_unique<WasapiCapture>();
	if (!capture->Start(g_renderIds[sel], true, err))
		return fail(std::wstring(S(L"Status", L"error_capture")) + L"\r\n" + err);

	std::unique_ptr<WasapiCapture> micCap;
	if (micSel > 0 && static_cast<size_t>(micSel - 1) < g_micIds.size())
	{
		micCap = std::make_unique<WasapiCapture>();
		if (!micCap->Start(g_micIds[micSel - 1], false, err))
		{
			// 麦克风起不来不再让整场录制失败：降级为只录播放设备
			AppendLogLine(L"mic capture failed, recording playback only:  " + err);
			micCap.reset();
			if (!fromAuto)
				warn = std::wstring(S(L"Status", L"warn_mic_fallback")) + L"\r\n" + err
				       + L"\r\n（" + S(L"Status", L"error_mic_hint") + L"）";
		}
		else
		{
			GetWindowTextW(g_comboMic, nameBuf, 256);
			g_micName = nameBuf;
			g_micId = g_micIds[micSel - 1];
		}
	}

	auto mixer = std::make_unique<AudioMixer>();
	auto writer = std::make_unique<Mp3Writer>();
	g_splitBase = MakeOutputBase();
	g_splitIndex = 0;
	g_outPath = g_splitBase + L".mp3";
	if (!writer->Open(g_outPath, capture->SampleRate(), capture->Channels(), BITRATE, err))
	{
		if (micCap)
			micCap->Stop();
		capture->Stop();
		return fail(std::wstring(S(L"Status", L"error_mp3")) + L"\r\n" + err + L"\r\n（" + S(L"Status", L"error_mp3_hint") + L"）");
	}

	g_capRate = capture->SampleRate();
	g_capChannels = capture->Channels();
	{
		std::lock_guard<std::mutex> lk(g_wmtx);
		g_writer = std::move(writer);
	}

	mixer->Open(g_capRate, g_capChannels,
		    [](const int16_t *d, uint32_t f)
		    {
			    std::lock_guard<std::mutex> lk(g_wmtx);
			    if (g_writer)
				    g_writer->Push(d, f);
		    });
	mixer->SetEchoCancellation(Button_GetCheck(g_chkAec) == BST_CHECKED);
	if (micCap)
		mixer->SetMicFormat(micCap->SampleRate(), micCap->Channels());

	AudioMixer *mx = mixer.get();
	HWND hwnd = g_hwnd;
	auto errFn = [hwnd](const std::wstring &msg)
	{
		PostMessageW(hwnd, WM_APP + 1, 0, reinterpret_cast<LPARAM>(new std::wstring(msg)));
	};
	capture->Begin([mx](const int16_t *d, uint32_t f) { mx->PushDesktop(d, f); }, errFn);
	if (micCap)
		micCap->Begin([mx](const int16_t *d, uint32_t f) { mx->PushMic(d, f); }, errFn);

	g_capture = std::move(capture);
	g_micCap = std::move(micCap);
	g_mixer = std::move(mixer);
	g_recording = true;
	g_startTick = GetTickCount64();
	g_segStartTick = g_startTick;
	g_startTime = Timestamp();
	AppendLog(L"Start  device: " + g_deviceName + L"  mic: " + g_micName + L"  file: " + g_outPath);
	if (g_splitMinutes > 0)
		AppendLog(L"auto split every " + std::to_wstring(g_splitMinutes) + L" min");

	SetWindowTextW(g_btnStart, S(L"UI", L"btn_stop"));
	EnableWindow(g_combo, FALSE);
	EnableWindow(g_comboMic, FALSE);
	EnableWindow(g_chkAec, FALSE);
	EnableWindow(g_btnRefresh, FALSE);
	EnableWindow(g_chkAutoSplit, FALSE);
	EnableWindow(g_comboSplit, FALSE);
	ShowRecordingStatus();
	if (!warn.empty())
		MessageBoxW(g_hwnd, warn.c_str(), S(L"About", L"title"), MB_ICONWARNING);
	return true;
}

void StopRecording(StopReason reason)
{
	if (!g_recording)
		return;
	g_recording = false;
	KillTimer(g_hwnd, 1);

	HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
	if (g_micCap)
		g_micCap->Stop();
	g_capture->Stop();
	std::unique_ptr<Mp3Writer> writer;
	{
		std::lock_guard<std::mutex> lk(g_wmtx);
		writer.swap(g_writer);
	}
	writer->Close();
	SetCursor(old);

	std::wstring endTime = Timestamp();
	const bool failed = writer->HasError() || !g_pendingError.empty();
	const std::wstring errText = writer->HasError() ? writer->Error() : g_pendingError;
	std::wstring status;
	if (failed)
	{
		status = std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"end") + L": " + endTime + L"\r\n" + S(L"Status", L"interrupted") + L"：" + errText;
		if (std::filesystem::exists(g_outPath))
			status += L"\r\n" + std::wstring(S(L"Status", L"file")) + L": " + g_outPath;
	}
	else
	{
		std::wstring size = L"?";
		std::error_code ec;
		const auto bytes = std::filesystem::file_size(g_outPath, ec);
		if (!ec)
		{
			wchar_t b[64];
			swprintf_s(b, L"%.1f MB", bytes / 1048576.0);
			size = b;
		}
		status = std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"end") + L": " + endTime + L"\r\n" +
			 S(L"Status", L"completed") + L"  " + S(L"Status", L"duration") + L" " +
			 FormatDuration(GetTickCount64() - g_startTick) + L"  " + S(L"Status", L"size") + L" " + size + L"\r\n" + S(L"Status", L"file") + L": " + g_outPath;
	}
	SetStatus(status);
	AppendLogLine(failed ? std::wstring(L"interrupted: ") + errText + L"  file: " + g_outPath
			     : std::wstring(L"completed  duration ") + FormatDuration(GetTickCount64() - g_startTick) + L"  file: " + g_outPath);
	g_pendingError.clear();
	if (g_splitIndex > 0)
		AppendLog(L"auto split wrote " + std::to_wstring(g_splitIndex + 1) + L" files");

	if (reason == StopReason::User && g_autoStarted)
	{
		std::wstring dummy;
		if (Button_GetCheck(g_chkAutoTeams) == BST_CHECKED && TeamsWatcher::MeetingActive(dummy))
		{
			g_autoSuppress = true;
			AppendLog(L"auto-record suppressed for the rest of this meeting (manual stop)");
		}
	}
	g_autoStarted = false;
	g_startRetries = 0;
	// 异常中断：让看门狗 2s 后重新自动开始；其余情况清零
	g_retryCountdown = (reason == StopReason::DeviceError) ? 1 : 0;

	g_micCap.reset();
	g_capture.reset();
	g_mixer.reset();
	g_writer.reset();
	++g_recordCount;
	wchar_t countBuf[32];
	swprintf_s(countBuf, L"%s: %d", S(L"UI", L"record_count"), g_recordCount);
	SetWindowTextW(g_recordCountText, countBuf);

	SetWindowTextW(g_btnStart, S(L"UI", L"btn_start"));
	EnableWindow(g_combo, TRUE);
	EnableWindow(g_comboMic, TRUE);
	EnableWindow(g_chkAec, TRUE);
	EnableWindow(g_btnRefresh, TRUE);
	EnableWindow(g_chkAutoSplit, TRUE);
	EnableWindow(g_comboSplit, TRUE);
}

// 采集报错后的自愈：保住已经写好的分段，换到可用的播放设备继续录。
// 结果是后续内容进入新的分段文件（同一场会议会出现多个文件），已录部分不受影响。
bool TryReconnectCapture()
{
	if (!g_recording || !g_capture)
		return false;
	AppendLog(L"capture error, trying to switch to another playback device");

	// 1) 只停采集线程，先不关编码器：已经录到的内容必须保住
	if (g_micCap)
		g_micCap->Stop();
	g_capture->Stop();

	// 2) 设备列表很可能已经变了
	RefreshDevicesKeepingSelection();

	// 3) 优先用会议正在使用的设备，其次用下拉框当前选择
	std::wstring app, devId, devName;
	TeamsWatcher::MeetingActive(app, devId, devName);
	int idx = FindRenderIndex(devId);
	if (idx < 0)
		idx = ComboBox_GetCurSel(g_combo);
	if (idx < 0 || static_cast<size_t>(idx) >= g_renderIds.size())
	{
		AppendLog(L"reconnect failed: no usable playback device");
		return false;
	}

	std::wstring err;
	auto cap = std::make_unique<WasapiCapture>();
	if (!cap->Start(g_renderIds[idx], true, err))
	{
		AppendLogLine(L"reconnect failed:  " + err);
		return false;
	}

	// 4) 麦克风起不来就降级为只录播放设备
	std::unique_ptr<WasapiCapture> micCap;
	if (!g_micId.empty())
	{
		int micIdx = -1;
		for (size_t i = 0; i < g_micIds.size(); ++i)
		{
			if (g_micIds[i] == g_micId)
			{
				micIdx = static_cast<int>(i);
				break;
			}
		}
		if (micIdx < 0)
		{
			AppendLog(L"mic device gone, recording playback only");
		}
		else
		{
			micCap = std::make_unique<WasapiCapture>();
			if (!micCap->Start(g_micIds[micIdx], false, err))
			{
				AppendLogLine(L"mic capture failed after reconnect, recording playback only:  " + err);
				micCap.reset();
			}
		}
	}

	// 5) 新的分段文件（沿用自动分割的命名规则）
	const std::wstring newPath = g_splitBase + L"_" + std::to_wstring(g_splitIndex + 1) + L".mp3";
	auto next = std::make_unique<Mp3Writer>();
	if (!next->Open(newPath, cap->SampleRate(), cap->Channels(), BITRATE, err))
	{
		AppendLogLine(L"reconnect failed (encoder):  " + err);
		cap->Stop();
		return false;
	}

	std::unique_ptr<Mp3Writer> prev;
	{
		std::lock_guard<std::mutex> lk(g_wmtx);
		prev.swap(g_writer);
		g_writer = std::move(next);
	}
	if (prev)
		prev->Close();

	// 6) 新混音器 + 新采集线程
	auto mixer = std::make_unique<AudioMixer>();
	mixer->Open(cap->SampleRate(), cap->Channels(),
		    [](const int16_t *d, uint32_t f)
		    {
			    std::lock_guard<std::mutex> lk(g_wmtx);
			    if (g_writer)
				    g_writer->Push(d, f);
		    });
	mixer->SetEchoCancellation(Button_GetCheck(g_chkAec) == BST_CHECKED);
	if (micCap)
		mixer->SetMicFormat(micCap->SampleRate(), micCap->Channels());

	AudioMixer *mx = mixer.get();
	HWND hwnd = g_hwnd;
	auto errFn = [hwnd](const std::wstring &msg)
	{
		PostMessageW(hwnd, WM_APP + 1, 0, reinterpret_cast<LPARAM>(new std::wstring(msg)));
	};
	cap->Begin([mx](const int16_t *d, uint32_t f) { mx->PushDesktop(d, f); }, errFn);
	if (micCap)
		micCap->Begin([mx](const int16_t *d, uint32_t f) { mx->PushMic(d, f); }, errFn);

	g_capture = std::move(cap); // 旧的（已 Stop）在这里被销毁
	g_micCap = std::move(micCap);
	g_mixer = std::move(mixer);

	g_capRate = g_capture->SampleRate();
	g_capChannels = g_capture->Channels();
	if (!devName.empty())
	{
		g_deviceName = devName;
	}
	else
	{
		wchar_t nameBuf[256]{};
		GetWindowTextW(g_combo, nameBuf, 256);
		g_deviceName = nameBuf;
	}
	if (!g_micCap)
		g_micName = S(L"UI", L"mic_none");
	g_outPath = newPath;
	++g_splitIndex;
	g_segStartTick = GetTickCount64();
	ShowRecordingStatus();
	AppendLog(L"reconnected  device: " + g_deviceName + L"  mic: " + g_micName + L"  file: " + g_outPath);
	g_pendingError.clear();
	return true;
}

void OnWatchTick()
{
	if (Button_GetCheck(g_chkAutoTeams) != BST_CHECKED)
		return;

	std::wstring app, devId, devName;
	if (TeamsWatcher::MeetingActive(app, devId, devName))
	{
		g_inactivePolls = 0;
		if (g_recording || g_autoSuppress)
			return;

		if (g_retryCountdown > 0)
		{
			--g_retryCountdown;
			return;
		}

		g_meetingApp = app;
		if (StartRecording(true, FindRenderIndex(devId), devName))
		{
			g_autoStarted = true;
			g_startRetries = 0;
			if (!devId.empty())
				AppendLog(L"auto-start on the meeting's active playback device:  " + devName);
		}
		else if (++g_startRetries > MAX_AUTO_START_RETRIES)
		{
			g_autoSuppress = true;
			g_startRetries = 0;
			AppendLog(L"auto-record gave up after 4 failed attempts for this meeting");
		}
		else
		{
			RefreshDevicesKeepingSelection();
			g_retryCountdown = kRetryTicks[g_startRetries - 1];
			AppendLog(L"auto-record will retry, attempt " + std::to_wstring(g_startRetries + 1) +
				  L" in " + std::to_wstring(g_retryCountdown * 2) + L"s");
		}
	}
	else
	{
		g_autoSuppress = false;
		g_startRetries = 0;
		g_retryCountdown = 0;
		if (g_recording && g_autoStarted)
		{
			if (++g_inactivePolls >= WATCH_GRACE_POLLS)
			{
				AppendLog(L"Meeting ended (Teams/WeChat/DingTalk/TencentMeeting), auto stop  file: " + g_outPath);
				StopRecording(StopReason::Auto);
				g_inactivePolls = 0;
				SetStatus(std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"end") + L": " + Timestamp() + L"\r\n" + S(L"Status", L"teams_stopped") + L"\r\n" + S(L"Status", L"file") + L": " + g_outPath);
			}
		}
		else
		{
			g_inactivePolls = 0;
		}
	}
}
