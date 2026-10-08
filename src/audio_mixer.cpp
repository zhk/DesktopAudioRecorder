#include "audio_mixer.h"

#include <algorithm>
#include <cmath>

void AudioMixer::Open(uint32_t rate, uint16_t channels, MixedFn fn)
{
	std::lock_guard<std::mutex> lk(m_mtx);
	m_rate = rate;
	m_channels = channels;
	m_fn = std::move(fn);
	m_micEnabled = false;
	m_micIn.clear();
	m_micPcm.clear();
	m_micPos = 0.0;
	m_aecOn = false;
}

void AudioMixer::SetMicFormat(uint32_t rate, uint16_t channels)
{
	std::lock_guard<std::mutex> lk(m_mtx);
	m_micRate = rate;
	m_micChannels = channels;
	m_micEnabled = (rate != 0);
	m_micStep = m_micEnabled ? static_cast<double>(rate) / m_rate : 1.0;
	if (m_micEnabled && m_aecOn)
		m_aec.Configure(m_rate, m_micChannels);
}

void AudioMixer::SetEchoCancellation(bool on)
{
	std::lock_guard<std::mutex> lk(m_mtx);
	m_aecOn = on;
	if (m_micEnabled)
		m_aec.Configure(m_rate, m_micChannels);
}

void AudioMixer::Close()
{
	std::lock_guard<std::mutex> lk(m_mtx);
	m_fn = nullptr;
	m_micIn.clear();
	m_micPcm.clear();
}

void AudioMixer::PushDesktop(const int16_t *data, uint32_t frames)
{
	std::lock_guard<std::mutex> lk(m_mtx);
	if (!m_fn)
		return;

	if (!m_micEnabled) {
		m_fn(data, frames); // 不开麦克风时直通，零开销
		return;
	}

	// 桌面流同时作为回声消除的参考信号
	if (m_aecOn)
		m_aec.PushReference(data, frames, m_channels);

	const size_t samples = static_cast<size_t>(frames) * m_channels;
	m_mixBuf.resize(samples);
	for (size_t i = 0; i < samples; ++i) {
		int32_t v = data[i];
		if (!m_micPcm.empty()) {
			v += m_micPcm.front();
			m_micPcm.pop_front();
		}
		// 饱和防削波
		m_mixBuf[i] = static_cast<int16_t>(v > 32767 ? 32767 : (v < -32768 ? -32768 : v));
	}
	m_fn(m_mixBuf.data(), frames);
}

void AudioMixer::PushMic(const int16_t *data, uint32_t frames)
{
	std::lock_guard<std::mutex> lk(m_mtx);
	if (!m_micEnabled || !m_fn || frames == 0)
		return;

	// 1) 追加原始输入（麦克风原生声道数，输入速率域）
	m_micIn.insert(m_micIn.end(), data, data + static_cast<size_t>(frames) * m_micChannels);

	// 2) 重采样到输出速率（线性插值，会议语音足够），保持原生声道数。
	//    统一经 m_rsBuf 中转：AEC 需要就地修改，而 deque 非连续存储。
	if (m_micRate == m_rate) {
		m_rsBuf.assign(m_micIn.begin(), m_micIn.end());
		m_micIn.clear();
		m_micPos = 0.0;
	} else {
		m_rsBuf.clear();
		size_t avail = m_micIn.size() / m_micChannels;
		while (m_micPos + 1.0 < static_cast<double>(avail)) {
			const size_t i0 = static_cast<size_t>(m_micPos);
			const double frac = m_micPos - i0;
			for (uint16_t ch = 0; ch < m_micChannels; ++ch) {
				const int32_t a = m_micIn[i0 * m_micChannels + ch];
				const int32_t b = m_micIn[(i0 + 1) * m_micChannels + ch];
				m_rsBuf.push_back(static_cast<int16_t>(a + (b - a) * frac));
			}
			m_micPos += m_micStep;
		}
		// 丢弃已完全消耗的输入帧，保留 floor(m_micPos) 帧供下次插值
		const size_t dropFrames = static_cast<size_t>(m_micPos);
		for (size_t i = 0; i < dropFrames * m_micChannels && !m_micIn.empty(); ++i)
			m_micIn.pop_front();
		m_micPos -= dropFrames;
	}
	int16_t *rsData = m_rsBuf.data();
	const uint32_t rsFrames = static_cast<uint32_t>(m_rsBuf.size() / m_micChannels);

	// 3) 回声消除（以桌面播放流为参考）
	if (m_aecOn && rsFrames > 0)
		m_aec.Process(rsData, rsFrames);

	// 4) 声道映射为输出声道数，进入待混音缓冲
	for (uint32_t f = 0; f < rsFrames; ++f) {
		if (m_micChannels == m_channels) {
			for (uint16_t ch = 0; ch < m_channels; ++ch)
				m_micPcm.push_back(rsData[static_cast<size_t>(f) * m_micChannels + ch]);
		} else if (m_channels == 2 && m_micChannels == 1) {
			int16_t v = rsData[f]; // 单声道麦克风 -> 双声道
			m_micPcm.push_back(v);
			m_micPcm.push_back(v);
		} else if (m_channels == 1 && m_micChannels == 2) {
			int32_t l = rsData[static_cast<size_t>(f) * 2];
			int32_t r = rsData[static_cast<size_t>(f) * 2 + 1];
			m_micPcm.push_back(static_cast<int16_t>((l + r) / 2));
		}
	}

	// 5) 时钟漂移兜底：缓冲超过约 0.5s 丢弃最旧数据
	const size_t cap = static_cast<size_t>(m_rate / 2) * m_channels;
	while (m_micPcm.size() > cap)
		m_micPcm.pop_front();
}
