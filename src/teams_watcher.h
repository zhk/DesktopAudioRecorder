#pragma once
#include <string>

// 会议检测：Teams、微信、钉钉桌面版
// 只有在通话/会议中才会在默认播放设备上保持活跃的音频会话
class TeamsWatcher
{
public:
	// 检测是否有会议活跃。app 返回匹配的进程名简称（teams/wechat/dingtalk）
	static bool MeetingActive(std::wstring &app);
};
