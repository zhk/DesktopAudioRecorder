#include "recorder.h"
#include "localization.h"
#include "logger.h"
#include "teams_watcher.h"

#include <windowsx.h>
#include <commctrl.h>
#include <filesystem>
#include <memory>
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

HWND g_hwnd{}, g_combo{}, g_comboMic{}, g_btnStart{}, g_btnRefresh{}, g_btnOpenFolder{}, g_status{}, g_chkAec{}, g_chkAutoTeams{}, g_recordCountText{};
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

std::wstring MakeOutputPath()
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
	swprintf_s(name, L"%s-%04d%02d%02d-%02d%02d%02d.mp3", g_meetingApp.empty() ? L"recording" : g_meetingApp.c_str(),
		   st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
	return (dir / name).wstring();
}

void SetStatus(const std::wstring &text)
{
	SetWindowTextW(g_status, text.c_str());
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

void ShowRecordingStatus()
{
	std::wstring text = std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"recording") + L"  " + S(L"Status", L"duration") + L" " + FormatDuration(GetTickCount64() - g_startTick);
	text += L"\r\n" + std::wstring(S(L"UI", L"label_playback")) + L" " + g_deviceName;
	text += L"\r\n" + std::wstring(S(L"UI", L"label_mic")) + L" " + g_micName;
	text += L"\r\n" + std::wstring(S(L"Status", L"file")) + L": " + g_outPath;
	SetStatus(text);
}

bool StartRecording(bool fromAuto)
{
	if (!fromAuto)
		g_meetingApp.clear();

	auto fail = [&](const std::wstring &msg)
	{
		if (fromAuto)
			SetStatus(msg);
		else
			MessageBoxW(g_hwnd, msg.c_str(), S(L"About", L"title"), MB_ICONERROR);
		return false;
	};

	const int sel = ComboBox_GetCurSel(g_combo);
	if (sel < 0 || static_cast<size_t>(sel) >= g_renderIds.size())
	{
		if (!fromAuto)
			MessageBoxW(g_hwnd, S(L"Status", L"error_no_device"), S(L"UI", L"app_title"), MB_ICONINFORMATION);
		return false;
	}
	const int micSel = ComboBox_GetCurSel(g_comboMic);

	wchar_t nameBuf[256]{};
	GetWindowTextW(g_combo, nameBuf, 256);
	g_deviceName = nameBuf;
	g_micName = S(L"UI", L"mic_none");

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
			capture->Stop();
			return fail(std::wstring(S(L"Status", L"error_mic")) + L"\r\n" + err + L"\r\n（" + S(L"Status", L"error_mic_hint") + L"）");
		}
		GetWindowTextW(g_comboMic, nameBuf, 256);
		g_micName = nameBuf;
	}

	auto mixer = std::make_unique<AudioMixer>();
	auto writer = std::make_unique<Mp3Writer>();
	g_outPath = MakeOutputPath();
	if (!writer->Open(g_outPath, capture->SampleRate(), capture->Channels(), BITRATE, err))
	{
		if (micCap)
			micCap->Stop();
		capture->Stop();
		return fail(std::wstring(S(L"Status", L"error_mp3")) + L"\r\n" + err + L"\r\n（" + S(L"Status", L"error_mp3_hint") + L"）");
	}

	Mp3Writer *w = writer.get();
	mixer->Open(capture->SampleRate(), capture->Channels(),
		    [w](const int16_t *d, uint32_t f) { w->Push(d, f); });
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
	g_writer = std::move(writer);
	g_recording = true;
	g_startTick = GetTickCount64();
	g_startTime = Timestamp();
	AppendLog(L"Start  device: " + g_deviceName + L"  mic: " + g_micName + L"  file: " + g_outPath);

	SetWindowTextW(g_btnStart, S(L"UI", L"btn_stop"));
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
	KillTimer(g_hwnd, 1);

	HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
	if (g_micCap)
		g_micCap->Stop();
	g_capture->Stop();
	g_writer->Close();
	SetCursor(old);

	std::wstring endTime = Timestamp();
	std::wstring status;
	if (g_writer->HasError())
	{
		status = std::wstring(S(L"Status", L"start")) + L": " + g_startTime + L"\r\n" + S(L"Status", L"end") + L": " + endTime + L"\r\n" + S(L"Status", L"interrupted") + L"：" + g_writer->Error();
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
	AppendLog(g_writer->HasError()
		  ? std::wstring(L"interrupted: ") + g_writer->Error() + L"  file: " + g_outPath
		  : std::wstring(L"completed  duration ") + FormatDuration(GetTickCount64() - g_startTick) + L"  file: " + g_outPath);

	if (g_autoStarted)
	{
		g_autoStarted = false;
		std::wstring dummy;
		if (Button_GetCheck(g_chkAutoTeams) == BST_CHECKED && TeamsWatcher::MeetingActive(dummy))
			g_autoSuppress = true;
	}

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
}

void OnWatchTick()
{
	if (Button_GetCheck(g_chkAutoTeams) != BST_CHECKED)
		return;

	std::wstring app;
	if (TeamsWatcher::MeetingActive(app))
	{
		g_inactivePolls = 0;
		if (!g_recording && !g_autoSuppress)
		{
			g_meetingApp = app;
			if (StartRecording(true))
				g_autoStarted = true;
			else
				g_autoSuppress = true;
		}
	}
	else
	{
		g_autoSuppress = false;
		if (g_recording && g_autoStarted)
		{
			if (++g_inactivePolls >= WATCH_GRACE_POLLS)
			{
				AppendLog(L"Meeting ended (Teams/WeChat/DingTalk), auto stop  file: " + g_outPath);
				StopRecording();
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
