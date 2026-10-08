#pragma once

#include "echo_canceller.h"

#include <cstdint>
#include <deque>
#include <functional>
#include <mutex>
#include <vector>

// 双路混音器：桌面(loopback)为主时钟基准，麦克风经线性插值重采样对齐、
// 可选回声消除后逐样本相加。两路 Push 可来自不同线程；输出格式 = 桌面格式。
// 设备时钟漂移的处理：麦克风缓冲设上限（超出丢最旧数据），欠载时补静音。
class AudioMixer {
public:
	using MixedFn = std::function<void(const int16_t *data, uint32_t frames)>;

	// 输出格式 = 桌面流的采样率/声道数
	void Open(uint32_t rate, uint16_t channels, MixedFn fn);
	// 设置麦克风格式；rate=0 表示不录麦克风（桌面流直通）
	void SetMicFormat(uint32_t rate, uint16_t channels);
	// 麦克风回声消除（以桌面流为参考）；默认关闭
	void SetEchoCancellation(bool on);

	void PushDesktop(const int16_t *data, uint32_t frames);
	void PushMic(const int16_t *data, uint32_t frames);
	void Close();

private:
	uint32_t m_rate = 0;
	uint16_t m_channels = 0;
	MixedFn m_fn;

	bool m_micEnabled = false;
	uint32_t m_micRate = 0;
	uint16_t m_micChannels = 0;
	double m_micStep = 1.0; // 每输出帧消耗的麦克风输入帧数
	double m_micPos = 0.0;  // m_micIn 内的小数帧位置

	bool m_aecOn = false;
	EchoCanceller m_aec;

	std::mutex m_mtx;
	std::deque<int16_t> m_micIn;  // 麦克风输入缓冲（输入速率，麦克风原生声道数）
	std::deque<int16_t> m_micPcm; // 麦克风处理后的缓冲（输出速率、输出声道数）
	std::vector<int16_t> m_rsBuf; // 重采样临时缓冲
	std::vector<int16_t> m_mixBuf;
};
