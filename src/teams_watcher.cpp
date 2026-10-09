#include "teams_watcher.h"

#include <windows.h>

#include <audiopolicy.h>
#include <mmdeviceapi.h>

namespace
{

bool IsTeamsProcess(DWORD pid)
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
	return _wcsicmp(name, L"teams.exe") == 0 || _wcsicmp(name, L"ms-teams.exe") == 0;
}

} // namespace

bool TeamsWatcher::MeetingActive()
{
	bool active = false;

	IMMDeviceEnumerator *enumerator = nullptr;
	HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
				      __uuidof(IMMDeviceEnumerator), reinterpret_cast<void **>(&enumerator));
	if (FAILED(hr))
		return false;

	do
	{
		IMMDevice *device = nullptr;
		if (FAILED(enumerator->GetDefaultAudioEndpoint(eRender, eConsole, &device)))
			break;

		IAudioSessionManager2 *manager = nullptr;
		hr = device->Activate(__uuidof(IAudioSessionManager2), CLSCTX_ALL, nullptr,
				      reinterpret_cast<void **>(&manager));
		device->Release();
		if (FAILED(hr))
			break;

		IAudioSessionEnumerator *sessionEnum = nullptr;
		hr = manager->GetSessionEnumerator(&sessionEnum);
		manager->Release();
		if (FAILED(hr))
			break;

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
					if (SUCCEEDED(ctl2->GetProcessId(&pid)) && pid != 0 && IsTeamsProcess(pid))
						active = true;
					ctl2->Release();
				}
			}
			ctl->Release();
		}
		sessionEnum->Release();
	} while (false);

	enumerator->Release();
	return active;
}
