#include "common.hpp";

#include <cstdint>
#include <processthreadsapi.h>
#include <psapi.h>


extern "C" {
    DWORD get_last_error() {
        return GetLastError();
    }

    bool get_version(LPWSTR fileName, DWORD out[4]) {
        bool ok = false;
        DWORD infoSize = GetFileVersionInfoSizeW(fileName, NULL);
        if (!infoSize) {
            return false;
        }
        uint8_t *buffer = new uint8_t[infoSize + 1];
        if (GetFileVersionInfoW(fileName, NULL, infoSize, buffer)) {
            VS_FIXEDFILEINFO *pFixedInfo;
            UINT infoLength;
            if (VerQueryValueW(buffer, L"\\", reinterpret_cast<LPVOID *>(&pFixedInfo), &infoLength)){
                out[0] = pFixedInfo->dwFileVersionMS >> 0x10;
                out[1] = pFixedInfo->dwFileVersionMS & 0xFFFF;
                out[2] = pFixedInfo->dwFileVersionLS >> 0x10;
                out[3] = pFixedInfo->dwFileVersionLS & 0xFFFF;
            }
            ok = true;
        }
        delete [] buffer;
        return ok;
    }

    enum GetProcessPathByWindowTitleResult: uint32_t {
        GetProcessPathByWindowTitleOk = 0,
        GetProcessPathByWindowTitleWindowNotFound = 1,
        GetProcessPathByWindowTitleGetWindowThreadProcessId = 2,
        GetProcessPathByWindowTitleOpenProcess = 3,
        GetProcessPathByWindowTitleGetModuleFileNameExW = 4
    };

    GetProcessPathByWindowTitleResult get_process_path_by_window_title(LPWSTR title, wchar_t *buffer, DWORD buffer_len) {
        using result = GetProcessPathByWindowTitleResult;
        auto hwnd = FindWindowW(NULL, title);
        if (hwnd == NULL) {
            return result::GetProcessPathByWindowTitleWindowNotFound;
        }

        DWORD process_id = 0;
        GetWindowThreadProcessId(hwnd, &process_id);
        if (process_id == 0) {
            return result::GetProcessPathByWindowTitleGetWindowThreadProcessId;
        }

        HANDLE process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, process_id);
        AutoCloseHandle process_handle(process);
        if (!process) {
            return result::GetProcessPathByWindowTitleOpenProcess;
        }

        auto len = GetModuleFileNameExW(process, NULL, buffer, buffer_len);
        if (!len) {
            return result::GetProcessPathByWindowTitleGetModuleFileNameExW;
        }

        return result::GetProcessPathByWindowTitleOk;
    }
}