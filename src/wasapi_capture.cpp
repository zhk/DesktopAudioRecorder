#include "wasapi_capture.h"

#include <audioclient.h>
#include <avrt.h>
#include <mmdeviceapi.h>

#include <algorithm>
#include <cmath>
#include <vector>

namespace {

// 本地定义子格式 GUID，避免链接 ksguid.lib
const GUID kSubtypePcm = {0x00000001, 0x0000, 0x0010, {0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}};
const GUID kSubtypeFloat = {0x00000003, 0x0000, 0x0010, {0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}};

std::wstring HrMessage(const wchar_t *what, HRESULT hr)
{
	wchar_t buf[256];
	swprintf_s(buf, L"%s (HRESULT=0x%08X)", what, static_cast<unsigned>(hr));
	return buf;
}

int16_t FloatToI16(float v)
{
	v = v > 1.f ? 1.f : (v < -1.f ? -1.f : v);
	return static_cast<int16_t>(lrintf(v * 32767.f));
}

struct Ctx {
	IMMDeviceEnumerator *enumerator = nullptr;
	IMMDevice *device = nullptr;
	IAudioClient *client = nullptr;
	IAudioCaptureClient *capture = nullptr;
	WAVEFORMATEX *wfx = nullptr;
	HANDLE evData = nullptr;
	bool isFloat = false;

	void Release()
	{
		if (capture)
			capture->Release();
		if (client)
			client->Release();
		if (device)
			device->Release();
		if (enumerator)
			enumerator->Release();
		if (wfx)
			CoTaskMemFree(wfx);
		if (evData)
			CloseHandle(evData);
		*this = Ctx{};
	}
};

bool InitLoopback(const std::wstring &deviceId, bool loopback, Ctx &c, std::wstring &error)
{
	HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
				      __uuidof(IMMDeviceEnumerator), reinterpret_cast<void **>(&c.enumerator));
	if (FAILED(hr)) {
		error = HrMessage(L"创建 MMDeviceEnumerator 失败", hr);
		return false;
	}

	const wchar_t *what = loopback ? L"播放" : L"录音";
	if (deviceId.empty())
		hr = c.enumerator->GetDefaultAudioEndpoint(loopback ? eRender : eCapture, eConsole, &c.device);
	else
		hr = c.enumerator->GetDevice(deviceId.c_str(), &c.device);
	if (FAILED(hr)) {
		error = HrMessage((std::wstring(L"打开") + what + L"设备失败").c_str(), hr);
		return false;
	}

	hr = c.device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, reinterpret_cast<void **>(&c.client));
	if (FAILED(hr)) {
		error = HrMessage(L"激活 IAudioClient 失败", hr);
		return false;
	}

	hr = c.client->GetMixFormat(&c.wfx);
	if (FAILED(hr)) {
		error = HrMessage(L"GetMixFormat 失败", hr);
		return false;
	}

	// 只接受 float32 / int16 的混音格式（现代 Windows 上几乎总是 float32）
	const WAVEFORMATEX *w = c.wfx;
	if (w->wFormatTag == WAVE_FORMAT_IEEE_FLOAT && w->wBitsPerSample == 32) {
		c.isFloat = true;
	} else if (w->wFormatTag == WAVE_FORMAT_PCM && w->wBitsPerSample == 16) {
		c.isFloat = false;
	} else if (w->wFormatTag == WAVE_FORMAT_EXTENSIBLE && w->cbSize >= 22) {
		const auto *ext = reinterpret_cast<const WAVEFORMATEXTENSIBLE *>(w);
		if (ext->SubFormat == kSubtypeFloat && w->wBitsPerSample == 32)
			c.isFloat = true;
		else if (ext->SubFormat == kSubtypePcm && w->wBitsPerSample == 16)
			c.isFloat = false;
		else {
			error = L"不支持的混音格式（仅支持 float32 / int16）";
			return false;
		}
	} else {
		error = L"不支持的混音格式（仅支持 float32 / int16）";
		return false;
	}

	c.evData = CreateEventW(nullptr, FALSE, FALSE, nullptr);
	if (!c.evData) {
		error = L"CreateEvent 失败";
		return false;
	}

	// 100ms 缓冲区 + 事件驱动，与 OBS win-wasapi 的事件模式一致
	DWORD flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
	if (loopback)
		flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
	hr = c.client->Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 100 * 10000, 0, c.wfx, nullptr);
	if (FAILED(hr)) {
		error = HrMessage(L"IAudioClient::Initialize 失败", hr);
		return false;
	}

	hr = c.client->SetEventHandle(c.evData);
	if (FAILED(hr)) {
		error = HrMessage(L"SetEventHandle 失败", hr);
		return false;
	}

	hr = c.client->GetService(__uuidof(IAudioCaptureClient), reinterpret_cast<void **>(&c.capture));
	if (FAILED(hr)) {
		error = HrMessage(L"获取 IAudioCaptureClient 失败", hr);
		return false;
	}

	return true;
}

} // namespace

WasapiCapture::~WasapiCapture()
{
	Stop();
}

bool WasapiCapture::Start(const std::wstring &deviceId, bool loopback, std::wstring &error)
{
	Stop();

	m_deviceId = deviceId;
	m_loopback = loopback;
	m_running = true;
	m_evReady = CreateEventW(nullptr, TRUE, FALSE, nullptr);
	m_evGo = CreateEventW(nullptr, TRUE, FALSE, nullptr);
	m_initOk = false;
	m_initError.clear();

	m_thread = std::thread(&WasapiCapture::Run, this);
	WaitForSingleObject(m_evReady, INFINITE);

	if (!m_initOk) {
		if (m_thread.joinable())
			m_thread.join();
		error = m_initError;
		return false;
	}
	return true;
}

void WasapiCapture::Begin(DataFn onData, ErrorFn onError)
{
	m_dataFn = std::move(onData);
	m_errorFn = std::move(onError);
	SetEvent(m_evGo);
}

void WasapiCapture::Stop()
{
	if (!m_thread.joinable())
		return;

	m_running = false;
	if (m_evGo)
		SetEvent(m_evGo); // 若还停在 Begin 之前，放行线程退出
	m_thread.join();

	if (m_evReady) {
		CloseHandle(m_evReady);
		m_evReady = nullptr;
	}
	if (m_evGo) {
		CloseHandle(m_evGo);
		m_evGo = nullptr;
	}
}

void WasapiCapture::Run()
{
	CoInitializeEx(nullptr, COINIT_MULTITHREADED);
	DWORD taskIndex = 0;
	HANDLE mmcss = AvSetMmThreadCharacteristicsW(L"Pro Audio", &taskIndex);

	Ctx c;
	std::wstring fatal;

	if (!InitLoopback(m_deviceId, m_loopback, c, m_initError)) {
		m_initOk = false;
		SetEvent(m_evReady);
	} else {
		m_rate = c.wfx->nSamplesPerSec;
		m_inChannels = c.wfx->nChannels;
		m_outChannels = m_inChannels > 2 ? 2 : m_inChannels;
		m_initOk = true;
		SetEvent(m_evReady);

		WaitForSingleObject(m_evGo, INFINITE);

		if (m_running) {
			HRESULT hr = c.client->Start();
			if (FAILED(hr))
				fatal = HrMessage(L"IAudioClient::Start 失败", hr);

			std::vector<int16_t> chunk;
			while (m_running && fatal.empty()) {
				if (WaitForSingleObject(c.evData, 500) != WAIT_OBJECT_0)
					continue;

				UINT32 packetFrames = 0;
				c.capture->GetNextPacketSize(&packetFrames);
				while (packetFrames > 0) {
					BYTE *data = nullptr;
					UINT32 frames = 0;
					DWORD flags = 0;
					hr = c.capture->GetBuffer(&data, &frames, &flags, nullptr, nullptr);
					if (hr == AUDCLNT_S_BUFFER_EMPTY)
						break;
					if (FAILED(hr)) {
						fatal = (hr == AUDCLNT_E_DEVICE_INVALIDATED)
								? L"音频设备已断开或失效"
								: HrMessage(L"GetBuffer 失败", hr);
						break;
					}

					if (frames > 0) {
						chunk.assign(static_cast<size_t>(frames) * m_outChannels, 0);
						if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT)) {
							const float *srcF = reinterpret_cast<const float *>(data);
							const int16_t *srcI = reinterpret_cast<const int16_t *>(data);
							auto get = [&](size_t idx) -> float {
								return c.isFloat ? srcF[idx] : srcI[idx] / 32768.f;
							};
							for (UINT32 f = 0; f < frames; ++f) {
								const size_t base = static_cast<size_t>(f) * m_inChannels;
								int16_t *out = &chunk[static_cast<size_t>(f) * m_outChannels];
								if (m_inChannels == m_outChannels) {
									for (uint16_t ch = 0; ch < m_inChannels; ++ch)
										out[ch] = FloatToI16(get(base + ch));
								} else {
									// 多声道降混为立体声：
									// 前置 L/R + 0.707*中置 + 环绕均摊，丢弃 LFE
									float l = get(base + 0);
									float r = get(base + 1);
									if (m_inChannels >= 3) {
										float ctr = get(base + 2);
										l += 0.7071f * ctr;
										r += 0.7071f * ctr;
									}
									for (uint16_t ch = 4; ch < m_inChannels; ++ch) {
										float s = get(base + ch);
										l += 0.3536f * s;
										r += 0.3536f * s;
									}
									out[0] = FloatToI16(l);
									out[1] = FloatToI16(r);
								}
							}
						}
						if (m_dataFn)
							m_dataFn(chunk.data(), frames);
					}

					c.capture->ReleaseBuffer(frames);
					c.capture->GetNextPacketSize(&packetFrames);
				}
			}
			c.client->Stop();
		}
	}

	c.Release();
	if (mmcss)
		AvRevertMmThreadCharacteristics(mmcss);
	CoUninitialize();

	if (!fatal.empty() && m_errorFn)
		m_errorFn(fatal);
}
