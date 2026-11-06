#pragma once
#include <windows.h>
#include <mmdeviceapi.h>
#include <memory>
#include <string>
#include <vector>

#include "audio_mixer.h"
#include "mp3_writer.h"
#include "wasapi_capture.h"

extern HWND g_hwnd, g_combo, g_comboMic, g_btnStart, g_btnRefresh, g_btnOpenFolder, g_status, g_chkAec, g_chkAutoTeams, g_recordCountText, g_chkAutoStart, g_chkAutoSplit, g_comboSplit;
extern int g_recordCount;
extern std::vector<std::wstring> g_renderIds;
extern std::vector<std::wstring> g_micIds;

extern std::unique_ptr<WasapiCapture> g_capture;
extern std::unique_ptr<WasapiCapture> g_micCap;
extern std::unique_ptr<AudioMixer> g_mixer;
extern std::unique_ptr<Mp3Writer> g_writer;
extern bool g_recording;
extern bool g_autoStarted;
extern bool g_autoSuppress;
extern int g_inactivePolls;
extern ULONGLONG g_startTick;
extern std::wstring g_startTime;
extern std::wstring g_outPath;
extern std::wstring g_deviceName;
extern std::wstring g_micName;
extern std::wstring g_meetingApp;

void SetStatus(const std::wstring &text);
void ShowRecordingStatus();
void FillDeviceCombo(HWND combo, std::vector<std::wstring> &ids, EDataFlow flow, bool addNone);
void RefreshDevices();
void RefreshDevicesKeepingSelection();
int FindRenderIndex(const std::wstring &deviceId);
enum class StopReason
{
	User,        // 用户点了停止按钮（唯一会抑制本场自动重开的情况）
	DeviceError, // 采集/编码器运行期错误
	Auto,        // 看门狗检测到会议结束
	Exiting,     // 程序退出
};
bool StartRecording(bool fromAuto, int renderOverride = -1, const std::wstring &nameOverride = std::wstring());
void StopRecording(StopReason reason);
void OnWatchTick();
void FillSplitCombo();
void SyncSplitFromControls();
void MaybeSplitOutput();
// 采集线程运行期错误：让 StopRecording 把它当“中断”写日志（编码器本身没出错时）
void NoteRecordError(const std::wstring &msg);
// 打开/关闭自动录制开关时重置看门狗状态
void ResetAutoWatchState();
// 采集报错后自愈：保住已录的分段，换设备继续录（同一场会议会出现多个文件）
bool TryReconnectCapture();
