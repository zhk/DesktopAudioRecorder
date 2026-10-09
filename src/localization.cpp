#include "localization.h"
#include <filesystem>

std::wstring g_lang;
std::map<std::wstring, std::wstring> g_str;

const wchar_t *S(const wchar_t *section, const wchar_t *key)
{
	auto it = g_str.find(std::wstring(section) + L"." + key);
	if (it != g_str.end())
		return it->second.c_str();
	return L"?";
}

std::wstring DetectSystemLocale()
{
	const LANGID lang = GetUserDefaultUILanguage();
	return PRIMARYLANGID(lang) == LANG_CHINESE_SIMPLIFIED ? L"zh-CN" : L"en-US";
}

std::wstring ExeDir()
{
	wchar_t buf[MAX_PATH]{};
	GetModuleFileNameW(nullptr, buf, MAX_PATH);
	return std::filesystem::path(buf).parent_path().wstring();
}

std::wstring ExeName()
{
	wchar_t buf[MAX_PATH]{};
	GetModuleFileNameW(nullptr, buf, MAX_PATH);
	return std::filesystem::path(buf).stem().wstring();
}

void LoadLangFile(const std::wstring &locale)
{
	g_str.clear();
	wchar_t iniPath[MAX_PATH]{};
	swprintf_s(iniPath, L"%s\\lang\\%s.ini", ExeDir().c_str(), locale.c_str());
	if (GetFileAttributesW(iniPath) == INVALID_FILE_ATTRIBUTES)
		return;

	HANDLE hFile = CreateFileW(iniPath, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
				   FILE_ATTRIBUTE_NORMAL, nullptr);
	if (hFile == INVALID_HANDLE_VALUE)
		return;
	DWORD fileSize = GetFileSize(hFile, nullptr);
	std::wstring content(fileSize / 2 + 1, L'\0');
	DWORD read = 0;
	ReadFile(hFile, &content[0], fileSize, &read, nullptr);
	CloseHandle(hFile);
	content.resize(read / 2);

	size_t pos = 0;
	if (content.size() > 0 && content[0] == 0xFEFF)
		pos = 1;

	std::wstring_view secName;
	auto addEntry = [&](std::wstring_view sec, std::wstring_view key, std::wstring_view val)
	{
		std::wstring v(val);
		size_t p;
		while ((p = v.find(L"\\n")) != std::wstring::npos)
			v.replace(p, 2, L"\r\n");
		g_str[std::wstring(sec) + L"." + std::wstring(key)] = std::move(v);
	};

	while (pos < content.size())
	{
		size_t eol = content.find(L'\n', pos);
		if (eol == std::wstring::npos)
			eol = content.size();
		size_t end = eol;
		while (end > pos && (content[end - 1] == L'\r' || content[end - 1] == L'\n'))
			--end;

		std::wstring_view line(content.data() + pos, end - pos);
		if (!line.empty())
		{
			if (line.front() == L'[' && line.back() == L']')
			{
				secName = line.substr(1, line.size() - 2);
			}
			else if (!secName.empty())
			{
				auto eq = line.find(L'=');
				if (eq != std::wstring_view::npos)
				{
					auto key = line.substr(0, eq);
					auto val = line.substr(eq + 1);
					addEntry(secName, key, val);
				}
			}
		}
		pos = eol + 1;
	}
}

void LoadConfig()
{
	wchar_t ini[MAX_PATH]{};
	swprintf_s(ini, L"%s\\%s.ini", ExeDir().c_str(), ExeName().c_str());
	wchar_t locale[16]{};
	GetPrivateProfileStringW(L"Language", L"locale", L"", locale, 16, ini);
	if (locale[0] == L'\0')
		g_lang = DetectSystemLocale();
	else
		g_lang = locale;
	LoadLangFile(g_lang);
}

void SaveConfig()
{
	wchar_t ini[MAX_PATH]{};
	swprintf_s(ini, L"%s\\%s.ini", ExeDir().c_str(), ExeName().c_str());
	WritePrivateProfileStringW(L"Language", L"locale", g_lang.c_str(), ini);
}
