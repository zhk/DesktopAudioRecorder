#pragma once

// Teams 会议检测：Teams（经典版 Teams.exe / 新版 ms-teams.exe）只有在
// 通话/会议中才会在默认播放设备上保持活跃的音频会话。会议窗口最小化、
// 隐藏均不影响检测（不依赖窗口标题）。
class TeamsWatcher {
public:
	// 当前是否存在 Teams 的活跃音频会话（≈ 正在会议中）
	static bool MeetingActive();
};
