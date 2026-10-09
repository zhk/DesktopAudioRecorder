#pragma once
#include <windows.h>
#include <string>

std::wstring Timestamp();
void AppendLog(const std::wstring &line);
std::wstring FormatDuration(ULONGLONG ms);
std::string WToUtf8(const std::wstring &w);
