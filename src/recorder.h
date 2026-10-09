#pragma once
#include <windows.h>
#include <mmdeviceapi.h>
#include <memory>
#include <string>
#include <vector>

#include "audio_mixer.h"
#include "mp3_writer.h"
#include "wasapi_capture.h"

extern HWND g_hwnd, g_combo, g_comboMic, g_btnStart, g_btnRefresh, g_btnOpenFolder, g_status, g_chkAec, g_chkAutoTeams, g_recordCountText;
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

void SetStatus(const std::wstring &text);
void ShowRecordingStatus();
void FillDeviceCombo(HWND combo, std::vector<std::wstring> &ids, EDataFlow flow, bool addNone);
void RefreshDevices();
bool StartRecording(bool fromAuto);
void StopRecording();
void OnWatchTick();
