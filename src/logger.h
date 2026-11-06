#pragma once
#include <windows.h>
#include <string>

std::wstring Timestamp();
void AppendLog(const std::wstring &line);
// 把多行文本压成单行后写日志（失败信息常含 \r\n）
void AppendLogLine(const std::wstring &line);
std::wstring FormatDuration(ULONGLONG ms);
std::string WToUtf8(const std::wstring &w);
