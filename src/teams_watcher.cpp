#include "teams_watcher.h"

#include <windows.h>

#include <audiopolicy.h>
#include <mmdeviceapi.h>

namespace
{

// PKEY_Device_FriendlyName
const PROPERTYKEY kPKeyDeviceFriendlyName = {{0xa45c254e, 0xdf1c, 0x4efd,
					     {0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0}},
					    14};

bool IsMeetingProcess(DWORD pid, std::wstring &app)
{
	HANDLE h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
	if (!h)
		return false;
	wchar_t path[MAX_PATH];
	DWORD size = MAX_PATH;
	const bool ok = QueryFullProcessImageNameW(h, 0, path, &size);
	CloseHandle(h);
	if (!ok)
		return false;

	const wchar_t *name = wcsrchr(path, L'\\');
	name = name ? name + 1 : path;

	if (_wcsicmp(name, L"teams.exe") == 0 || _wcsicmp(name, L"ms-teams.exe") == 0)
	{
		app = L"Teams";
		return true;
	}
	if (_wcsicmp(name, L"wechat.exe") == 0 || _wcsicmp(name, L"weixin.exe") == 0)
	{
		app = L"WeChat";
		return true;
	}
	if (_wcsicmp(name, L"dingtalk.exe") == 0 || _wcsicmp(name, L"dingtalklite.exe") == 0 || _wcsicmp(name, L"DingTalk.exe") == 0)
	{
		app = L"DingTalk";
		return true;
	}
	if (_wcsicmp(name, L"wemeet.exe") == 0 || _wcsicmp(name, L"wemeetapp.exe") == 0 || _wcsicmp(name, L"wemeetlauncher.exe") == 0)
	{
		app = L"TencentMeeting";
		return true;
	}
	return false;
}

std::wstring DeviceName(IMMDevice *device)
{
	std::wstring name;
	IPropertyStore *store = nullptr;
	if (FAILED(device->OpenPropertyStore(STGM_READ, &store)))
		return name;
	PROPVARIANT pv;
	PropVariantInit(&pv);
	if (SUCCEEDED(store->GetValue(kPKeyDeviceFriendlyName, &pv)) && pv.vt == VT_LPWSTR && pv.pwszVal)
		name = pv.pwszVal;
	PropVariantClear(&pv);
	store->Release();
	return name;
}

} // namespace

bool TeamsWatcher::MeetingActive(std::wstring &app, std::wstring &deviceId, std::wstring &deviceName)
{
	bool active = false;
	app.clear();
	deviceId.clear();
	deviceName.clear();

	IMMDeviceEnumerator *enumerator = nullptr;
	HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
				      __uuidof(IMMDeviceEnumerator), reinterpret_cast<void **>(&enumerator));
	if (FAILED(hr))
		return false;

	// 枚举所有活跃的播放设备，而非只查默认设备（蓝牙/USB 音频等可能是活跃但非默认）
	IMMDeviceCollection *deviceCollection = nullptr;
	if (FAILED(enumerator->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &deviceCollection)))
	{
		enumerator->Release();
		return false;
	}

	UINT deviceCount = 0;
	deviceCollection->GetCount(&deviceCount);

	for (UINT d = 0; d < deviceCount && !active; ++d)
	{
		IMMDevice *device = nullptr;
		if (FAILED(deviceCollection->Item(d, &device)))
			continue;

		IAudioSessionManager2 *manager = nullptr;
		hr = device->Activate(__uuidof(IAudioSessionManager2), CLSCTX_ALL, nullptr,
				      reinterpret_cast<void **>(&manager));
		if (FAILED(hr))
		{
			device->Release();
			continue;
		}

		IAudioSessionEnumerator *sessionEnum = nullptr;
		hr = manager->GetSessionEnumerator(&sessionEnum);
		manager->Release();
		if (FAILED(hr))
		{
			device->Release();
			continue;
		}

		int count = 0;
		sessionEnum->GetCount(&count);
		for (int i = 0; i < count && !active; ++i)
		{
			IAudioSessionControl *ctl = nullptr;
			if (FAILED(sessionEnum->GetSession(i, &ctl)))
				continue;

			AudioSessionState state;
			if (SUCCEEDED(ctl->GetState(&state)) && state == AudioSessionStateActive)
			{
				IAudioSessionControl2 *ctl2 = nullptr;
				if (SUCCEEDED(ctl->QueryInterface(__uuidof(IAudioSessionControl2),
								  reinterpret_cast<void **>(&ctl2))))
				{
					DWORD pid = 0;
					if (SUCCEEDED(ctl2->GetProcessId(&pid)) && pid != 0
					    && IsMeetingProcess(pid, app))
					{
						active = true;
						// 这台设备就是会议音频当前所在的输出设备
						LPWSTR id = nullptr;
						if (SUCCEEDED(device->GetId(&id)) && id)
						{
							deviceId = id;
							CoTaskMemFree(id);
						}
						deviceName = DeviceName(device);
					}
					ctl2->Release();
				}
			}
			ctl->Release();
		}
		sessionEnum->Release();
		device->Release();
	}

	deviceCollection->Release();
	enumerator->Release();
	return active;
}
