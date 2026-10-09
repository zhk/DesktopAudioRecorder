#pragma once
#include <windows.h>
#include <string>
#include <map>

extern std::wstring g_lang;
extern std::map<std::wstring, std::wstring> g_str;

const wchar_t *S(const wchar_t *section, const wchar_t *key);
std::wstring ExeDir();
std::wstring ExeName();
std::wstring DetectSystemLocale();
void LoadLangFile(const std::wstring &locale);
void LoadConfig();
void SaveConfig();
