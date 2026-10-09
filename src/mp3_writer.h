#pragma once

#include <condition_variable>
#include <cstdint>
#include <deque>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

struct IMFSinkWriter;

// Media Foundation Sink Writer 封装：PCM16 -> MP3 文件。
// 写入在独立线程进行，Push() 只做入队，采集线程不会被编码阻塞。
//
// 使用流程：
//   Mp3Writer w;
//   w.Open(path, 48000, 2, 192000, err);
//   w.Push(data, frames);   // 可来自其他线程
//   w.Close();              // 冲刷队列并 Finalize
class Mp3Writer
{
public:
	Mp3Writer() = default;
	~Mp3Writer();
	Mp3Writer(const Mp3Writer &) = delete;
	Mp3Writer &operator=(const Mp3Writer &) = delete;

	bool Open(const std::wstring &path, uint32_t sampleRate, uint16_t channels, uint32_t bitrateBps,
		  std::wstring &error);
	void Push(const int16_t *data, size_t frames);
	void Close();

	bool HasError() const { return m_failed; }
	const std::wstring &Error() const { return m_error; }

private:
	void Run();
	void WriteChunk(const int16_t *data, size_t frames);

	IMFSinkWriter *m_sink = nullptr;
	unsigned long m_stream = 0;
	uint32_t m_rate = 0;
	uint16_t m_channels = 0;
	int64_t m_time = 0; // 已写入时长，单位 100ns
	std::wstring m_path;

	std::thread m_thread;
	std::mutex m_mtx;
	std::condition_variable m_cv;
	std::deque<std::vector<int16_t>> m_queue;
	bool m_running = false;
	bool m_open = false;

	bool m_failed = false;
	std::wstring m_error;
};
