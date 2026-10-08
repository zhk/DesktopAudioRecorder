#include "echo_canceller.h"

#include <cmath>

void EchoCanceller::Configure(uint32_t sampleRate, uint16_t channels, uint32_t tailMs, float step)
{
	m_rate = sampleRate;
	m_ch = channels ? channels : 1;
	m_w = sampleRate * tailMs / 1000;
	m_mu = step;

	m_refFifo.clear();
	m_refCap = sampleRate / 4; // 250ms：参考流快于麦克风时丢弃最旧数据

	m_win.assign(m_w, 0.f);
	m_pos = 0;
	m_eps = m_w * 100.0;
	m_norm = m_eps;

	m_h.assign(m_ch, std::vector<float>(m_w, 0.f));

	m_pRef = 0.0;
	m_pMic = 0.0;
	m_k = 1.0 - exp(-1.0 / (sampleRate * 0.05)); // ~50ms 时间常数
	m_hold = 0;
	m_holdMax = sampleRate / 1000 * 150; // 150ms 挂起
}

void EchoCanceller::Reset()
{
	m_refFifo.clear();
	std::fill(m_win.begin(), m_win.end(), 0.f);
	m_pos = 0;
	m_norm = m_eps;
	for (auto &h : m_h)
		std::fill(h.begin(), h.end(), 0.f);
	m_pRef = 0.0;
	m_pMic = 0.0;
	m_hold = 0;
}

void EchoCanceller::PushReference(const int16_t *data, uint32_t frames, uint16_t refChannels)
{
	if (m_w == 0 || refChannels == 0)
		return;

	for (uint32_t f = 0; f < frames; ++f) {
		float x;
		if (refChannels == 1) {
			x = static_cast<float>(data[f]);
		} else {
			int32_t l = data[static_cast<size_t>(f) * refChannels];
			int32_t r = data[static_cast<size_t>(f) * refChannels + 1];
			x = (l + r) * 0.5f;
		}
		m_refFifo.push_back(x);
	}
	while (m_refFifo.size() > m_refCap)
		m_refFifo.pop_front();
}

void EchoCanceller::Process(int16_t *mic, uint32_t frames)
{
	if (m_w == 0)
		return;

	for (uint32_t f = 0; f < frames; ++f) {
		// 逐样本取参考，保证参考/麦克风 1:1 对齐消费
		float x = 0.f;
		if (!m_refFifo.empty()) {
			x = m_refFifo.front();
			m_refFifo.pop_front();
		}

		const float old = m_win[m_pos];
		m_norm += static_cast<double>(x) * x - static_cast<double>(old) * old;
		if (m_norm < m_eps)
			m_norm = m_eps;
		m_win[m_pos] = x;
		if (++m_pos == m_w)
			m_pos = 0;

		// 没有播放声音时不存在回声，直通（也省 CPU）
		if (m_norm <= m_eps * 1.01)
			continue;

		// 双讲检测（Geigel 功率比 + 挂起）：麦克风功率显著高于参考功率
		// 说明本地在说话（回声是参考的衰减泄漏，正常不会超过该比例）
		{
			const double s0 = static_cast<double>(mic[static_cast<size_t>(f) * m_ch]);
			m_pMic += (s0 * s0 - m_pMic) * m_k;
		}
		m_pRef += (static_cast<double>(x) * x - m_pRef) * m_k;
		if (m_pMic > m_dtdRatio * m_pRef)
			m_hold = m_holdMax;
		else if (m_hold > 0)
			--m_hold;
		const bool freeze = m_hold > 0;

		for (uint16_t ch = 0; ch < m_ch; ++ch) {
			const size_t si = static_cast<size_t>(f) * m_ch + ch;
			const float s = static_cast<float>(mic[si]);
			float *h = m_h[ch].data();

			// 回声估计：y = h · x；环形缓冲满后 m_pos 处即最旧样本，
			// h[i] 对应窗口内第 i 旧（i=0 最旧，i=m_w-1 最新）的参考样本
			float y = 0.f;
			uint32_t ri = m_pos;
			for (uint32_t i = 0; i < m_w; ++i) {
				y += h[i] * m_win[ri];
				if (++ri == m_w)
					ri = 0;
			}

			const float e = s - y; // 残差 = 去回声后的信号

			// NLMS 系数更新（双讲时冻结，避免语音干扰滤波器收敛）
			if (!freeze) {
				const float g = static_cast<float>(m_mu * e / (m_norm + 1e-9));
				ri = m_pos;
				for (uint32_t i = 0; i < m_w; ++i) {
					h[i] += g * m_win[ri];
					if (++ri == m_w)
						ri = 0;
				}
			}

			const int32_t v = static_cast<int32_t>(lrintf(e));
			mic[si] = static_cast<int16_t>(v > 32767 ? 32767 : (v < -32768 ? -32768 : v));
		}
	}
}
