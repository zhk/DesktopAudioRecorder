#include "mp3_writer.h"

#include <windows.h>

#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>

namespace {

std::wstring HrMessage(const wchar_t *what, HRESULT hr)
{
	wchar_t buf[256];
	swprintf_s(buf, L"%s (HRESULT=0x%08X)", what, static_cast<unsigned>(hr));
	return buf;
}

} // namespace

Mp3Writer::~Mp3Writer()
{
	Close();
}

bool Mp3Writer::Open(const std::wstring &path, uint32_t sampleRate, uint16_t channels, uint32_t bitrateBps,
		     std::wstring &error)
{
	Close();

	HRESULT hr = MFStartup(MF_VERSION, MFSTARTUP_FULL);
	if (FAILED(hr)) {
		error = HrMessage(L"MFStartup 失败", hr);
		return false;
	}

	IMFMediaType *outType = nullptr;
	IMFMediaType *inType = nullptr;
	bool ok = false;

	hr = MFCreateSinkWriterFromURL(path.c_str(), nullptr, nullptr, &m_sink);
	if (FAILED(hr)) {
		error = HrMessage(L"创建输出文件失败", hr);
		goto fail;
	}

	MFCreateMediaType(&outType);
	outType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
	outType->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_MP3);
	outType->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, sampleRate);
	outType->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, channels);
	outType->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrateBps / 8);
	outType->SetUINT32(MF_MT_AUDIO_BLOCK_ALIGNMENT, 1);
	// 注意：不要设置 MF_MT_AUDIO_BITS_PER_SAMPLE 等额外属性，
	// 内置 MP3 编码器枚举的输出类型没有该属性，多了会导致匹配失败
	hr = m_sink->AddStream(outType, &m_stream);
	if (FAILED(hr)) {
		error = HrMessage(L"AddStream(MP3) 失败，系统可能缺少内置 MP3 编码器", hr);
		goto fail;
	}

	MFCreateMediaType(&inType);
	inType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
	inType->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
	inType->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, sampleRate);
	inType->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, channels);
	inType->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
	inType->SetUINT32(MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2);
	inType->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, sampleRate * channels * 2);
	inType->SetUINT32(MF_MT_AUDIO_VALID_BITS_PER_SAMPLE, 16);
	inType->SetUINT32(MF_MT_ALL_SAMPLES_INDEPENDENT, TRUE);
	hr = m_sink->SetInputMediaType(m_stream, inType, nullptr);
	if (FAILED(hr)) {
		error = HrMessage(L"设置编码器输入格式失败", hr);
		goto fail;
	}

	hr = m_sink->BeginWriting();
	if (FAILED(hr)) {
		error = HrMessage(L"BeginWriting 失败", hr);
		goto fail;
	}

	m_rate = sampleRate;
	m_channels = channels;
	m_time = 0;
	m_path = path;
	m_failed = false;
	m_error.clear();
	m_running = true;
	m_open = true;
	m_thread = std::thread(&Mp3Writer::Run, this);
	ok = true;

fail:
	if (outType)
		outType->Release();
	if (inType)
		inType->Release();
	if (!ok) {
		if (m_sink) {
			m_sink->Release();
			m_sink = nullptr;
		}
		MFShutdown();
	}
	return ok;
}

void Mp3Writer::Push(const int16_t *data, size_t frames)
{
	if (!m_open || m_failed)
		return;

	std::lock_guard<std::mutex> lk(m_mtx);
	// 背压保护：积压超过约 5 秒（10ms 一包计）直接丢弃，避免内存膨胀
	if (m_queue.size() > 500)
		return;
	m_queue.emplace_back(data, data + frames * m_channels);
	m_cv.notify_one();
}

void Mp3Writer::Run()
{
	while (true) {
		std::vector<int16_t> chunk;
		{
			std::unique_lock<std::mutex> lk(m_mtx);
			m_cv.wait(lk, [&] { return !m_queue.empty() || !m_running; });
			if (m_queue.empty()) // 即 !m_running 且队列已排空
				break;
			chunk = std::move(m_queue.front());
			m_queue.pop_front();
		}
		WriteChunk(chunk.data(), chunk.size() / m_channels);
	}
}

void Mp3Writer::WriteChunk(const int16_t *data, size_t frames)
{
	if (m_failed || frames == 0)
		return;

	const DWORD bytes = static_cast<DWORD>(frames * m_channels * sizeof(int16_t));
	IMFMediaBuffer *buffer = nullptr;
	IMFSample *sample = nullptr;

	HRESULT hr = MFCreateMemoryBuffer(bytes, &buffer);
	if (SUCCEEDED(hr)) {
		BYTE *dst = nullptr;
		hr = buffer->Lock(&dst, nullptr, nullptr);
		if (SUCCEEDED(hr)) {
			memcpy(dst, data, bytes);
			buffer->Unlock();
			buffer->SetCurrentLength(bytes);
		}
	}
	if (SUCCEEDED(hr))
		hr = MFCreateSample(&sample);
	if (SUCCEEDED(hr))
		hr = sample->AddBuffer(buffer);
	if (SUCCEEDED(hr)) {
		const int64_t duration = static_cast<int64_t>(frames) * 10000000LL / m_rate;
		sample->SetSampleTime(m_time);
		sample->SetSampleDuration(duration);
		m_time += duration;
		hr = m_sink->WriteSample(m_stream, sample);
	}

	if (buffer)
		buffer->Release();
	if (sample)
		sample->Release();

	if (FAILED(hr) && !m_failed) {
		m_failed = true;
		m_error = HrMessage(L"MP3 写入失败", hr);
	}
}

void Mp3Writer::Close()
{
	if (!m_open)
		return;

	{
		std::lock_guard<std::mutex> lk(m_mtx);
		m_running = false;
	}
	m_cv.notify_one();
	if (m_thread.joinable())
		m_thread.join();

	if (!m_failed && m_time == 0) {
		// MP3 sink 没有收到任何样本时 Finalize 必然失败（空流），直接删掉空文件
		m_failed = true;
		m_error = L"未捕获到任何音频数据（该设备可能没有在播放声音），空文件已删除";
	}

	if (!m_failed) {
		HRESULT hr = m_sink->Finalize();
		if (FAILED(hr)) {
			m_failed = true;
			m_error = HrMessage(L"Finalize 失败", hr);
		}
	}
	m_sink->Release();
	m_sink = nullptr;
	MFShutdown();
	m_open = false;

	if (m_failed && m_time == 0)
		DeleteFileW(m_path.c_str());
}
