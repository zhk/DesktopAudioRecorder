#include "logger.h"
#include <cstdio>
#include <filesystem>

std::wstring Timestamp()
{
	SYSTEMTIME st{};
	GetLocalTime(&st);
	wchar_t buf[32];
	swprintf_s(buf, L"[%04d-%02d-%02d %02d:%02d:%02d]", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
	return buf;
}

std::string WToUtf8(const std::wstring &w)
{
	int len = WideCharToMultiByte(CP_UTF8, 0, w.c_str(), -1, nullptr, 0, nullptr, nullptr);
	std::string s(len, '\0');
	WideCharToMultiByte(CP_UTF8, 0, w.c_str(), -1, &s[0], len, nullptr, nullptr);
	while (!s.empty() && s.back() == '\0') s.pop_back();
	return s;
}

void AppendLog(const std::wstring &line)
{
	wchar_t exePath[MAX_PATH]{};
	GetModuleFileNameW(nullptr, exePath, MAX_PATH);
	SYSTEMTIME st{};
	GetLocalTime(&st);
	wchar_t logName[128];
	swprintf_s(logName, L"%s-%04d%02d.log", std::filesystem::path(exePath).stem().c_str(), st.wYear, st.wMonth);
	auto logPath = std::filesystem::path(exePath).parent_path() / logName;

	bool needBom = !std::filesystem::exists(logPath) || std::filesystem::file_size(logPath) == 0;
	std::string utf8 = WToUtf8(Timestamp() + L" " + line + L"\n");
	FILE *f = nullptr;
	_wfopen_s(&f, logPath.c_str(), needBom ? L"wb" : L"ab");
	if (f)
	{
		if (needBom)
		{
			unsigned char bom[] = { 0xEF, 0xBB, 0xBF };
			fwrite(bom, 1, 3, f);
		}
		fwrite(utf8.c_str(), 1, utf8.size(), f);
		fclose(f);
	}
}

std::wstring FormatDuration(ULONGLONG ms)
{
	const ULONGLONG s = ms / 1000;
	wchar_t buf[64];
	if (s >= 3600)
		swprintf_s(buf, L"%llu:%02llu:%02llu", s / 3600, (s / 60) % 60, s % 60);
	else
		swprintf_s(buf, L"%02llu:%02llu", s / 60, s % 60);
	return buf;
}
