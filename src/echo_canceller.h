#pragma once

#include <cstdint>
#include <deque>
#include <vector>

// 简易 NLMS（归一化最小均方）自适应回声消除，带双讲检测（DTD）。
//
// 原理：桌面播放流是已知的（loopback 参考信号），自适应滤波器学习
// “扬声器 -> 房间 -> 麦克风”的泄漏路径，从麦克风信号中减去回声估计。
// 双讲（本地说话）时语音对自适应算法是干扰，检测到后冻结系数更新，
// 仅用已收敛的滤波器继续做回声相减。
//
// 关键约束：参考流与麦克风流按样本 1:1 对齐消费（两者的相对偏移是常数，
// 由滤波器窗口吸收）；因此参考信号先进入 FIFO，Process 时逐样本取出。
// 两设备时钟漂移通过 FIFO 上限丢旧数据兜底。
class EchoCanceller
{
public:
	// channels 为麦克风声道数（1 或 2），tailMs 为回声窗口长度
	void Configure(uint32_t sampleRate, uint16_t channels, uint32_t tailMs = 100, float step = 0.1f);
	void Reset();

	// 追加桌面播放流到参考 FIFO（任意声道数，内部混缩为单声道）
	void PushReference(const int16_t *data, uint32_t frames, uint16_t refChannels);
	// 就地处理麦克风流（声道数 = Configure 的 channels），逐样本消费参考 FIFO
	void Process(int16_t *mic, uint32_t frames);

	uint32_t Tail() const { return m_w; }

private:
	uint32_t m_rate = 0;
	uint16_t m_ch = 0;
	uint32_t m_w = 0; // 滤波器长度（taps）
	float m_mu = 0.25f;

	std::deque<float> m_refFifo;  // 待消费的参考样本（单声道，输出速率域）
	size_t m_refCap = 0;          // FIFO 上限（时钟漂移兜底）

	std::vector<float> m_win;     // NLMS 窗口环形缓冲，长度 m_w
	uint32_t m_pos = 0;           // 下一个写入位置（== 最旧样本位置）
	double m_norm = 1e-6;         // 窗口内参考能量，增量维护
	double m_eps = 1.0;           // 能量下限：低于此值认为无播放声音，直通不处理

	std::vector<std::vector<float>> m_h; // 每声道一组滤波器系数

	// 双讲检测：指数平滑的参考/麦克风短时功率 + 挂起计数
	double m_pRef = 0.0;
	double m_pMic = 0.0;
	double m_k = 0.0;         // 平滑系数（约 50ms 时间常数）
	uint32_t m_hold = 0;      // 双讲挂起剩余样本数
	uint32_t m_holdMax = 0;   // 挂起时长（约 150ms）
	double m_dtdRatio = 0.25; // Geigel 功率比阈值（幅度比 0.5）
};
