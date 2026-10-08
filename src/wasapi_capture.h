#pragma once

#include <windows.h>

#include <atomic>
#include <cstdint>
#include <functional>
#include <string>
#include <thread>

// WASAPI 采集，支持两种模式：
//   loopback=true  —— 录制播放设备正在输出的声音（桌面音频回录）
//   loopback=false —— 录制采集设备输入的声音（麦克风）
// 逻辑参考 OBS 的 plugins/win-wasapi，去掉对 libobs 的依赖。
//
// 使用流程：
//   WasapiCapture cap;
//   cap.Start(deviceId, loopback, err);  // 阻塞直到设备初始化成功/失败
//   cap.Begin(onData, onError);          // 真正开始采集，PCM16 数据从回调吐出
//   ...
//   cap.Stop();                          // 停止并回收线程
class WasapiCapture {
public:
	// 交错存放的 PCM16 数据，frames 为每声道帧数
	using DataFn = std::function<void(const int16_t *data, uint32_t frames)>;
	// 运行期致命错误（如设备被拔出），在采集线程触发
	using ErrorFn = std::function<void(const std::wstring &message)>;

	WasapiCapture() = default;
	~WasapiCapture();
	WasapiCapture(const WasapiCapture &) = delete;
	WasapiCapture &operator=(const WasapiCapture &) = delete;

	// 启动采集线程并初始化设备，阻塞直到初始化完成。
	// deviceId 为空时使用默认设备；loopback 选择录制方向。
	bool Start(const std::wstring &deviceId, bool loopback, std::wstring &error);

	// 设置回调并让采集线程真正开始工作（Start 成功后调用）。
	void Begin(DataFn onData, ErrorFn onError);

	// 停止采集并回收线程，可重复调用。
	void Stop();

	uint32_t SampleRate() const { return m_rate; }
	// 输出声道数：设备多于 2 声道时会降混为立体声
	uint16_t Channels() const { return m_outChannels; }

private:
	void Run();

	std::wstring m_deviceId;
	bool m_loopback = true;
	std::thread m_thread;
	HANDLE m_evReady = nullptr; // 设备初始化完成（成败都会触发）
	HANDLE m_evGo = nullptr;    // Begin() 触发，开始采集
	std::atomic<bool> m_running{false};

	bool m_initOk = false;
	std::wstring m_initError;

	uint32_t m_rate = 0;
	uint16_t m_inChannels = 0;
	uint16_t m_outChannels = 0;

	DataFn m_dataFn;
	ErrorFn m_errorFn;
};
