#pragma once
#include <string>

// 会议检测：Teams、微信、钉钉、腾讯会议桌面版
// 只有在通话/会议中才会在活跃播放设备上保持活跃的音频会话
class TeamsWatcher
{
public:
	// 检测是否有会议活跃。app = 匹配到的 App 简称。
	// deviceId / deviceName = 会议音频当前实际所在的播放设备（自动录制据此选对设备；
	// 检测不到时留空）。
	static bool MeetingActive(std::wstring &app, std::wstring &deviceId, std::wstring &deviceName);

	// 只关心“是否在开会”的调用方用这个重载
	static bool MeetingActive(std::wstring &app)
	{
		std::wstring id, name;
		return MeetingActive(app, id, name);
	}
};
