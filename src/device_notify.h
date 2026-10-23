#pragma once
#include <windows.h>
#include <mmdeviceapi.h>
#include <string>

// 音频设备变更通知，默认播放/录制设备变化时自动刷新下拉框
class DeviceNotify : public IMMNotificationClient
{
	long m_ref = 1;
public:
	DeviceNotify() = default;

	// IMMNotificationClient
	HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void **ppv) override
	{
		if (riid == __uuidof(IMMNotificationClient) || riid == IID_IUnknown)
		{
			*ppv = static_cast<IMMNotificationClient *>(this);
			AddRef();
			return S_OK;
		}
		*ppv = nullptr;
		return E_NOINTERFACE;
	}
	ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&m_ref); }
	ULONG STDMETHODCALLTYPE Release() override
	{
		ULONG r = InterlockedDecrement(&m_ref);
		if (r == 0) delete this;
		return r;
	}

	HRESULT STDMETHODCALLTYPE OnDefaultDeviceChanged(EDataFlow flow, ERole role, LPCWSTR) override
	{
		if (role == eConsole && (flow == eRender || flow == eCapture))
			PostMessageW(GetForegroundWindow(), WM_APP + 4, 0, 0);
		return S_OK;
	}
	HRESULT STDMETHODCALLTYPE OnDeviceAdded(LPCWSTR) override { return S_OK; }
	HRESULT STDMETHODCALLTYPE OnDeviceRemoved(LPCWSTR) override { return S_OK; }
	HRESULT STDMETHODCALLTYPE OnDeviceStateChanged(LPCWSTR, DWORD) override { return S_OK; }
	HRESULT STDMETHODCALLTYPE OnPropertyValueChanged(LPCWSTR, const PROPERTYKEY) override { return S_OK; }
};