#define WIN32_LEAN_AND_MEAN
#include <Windows.h>

class AutoCloseHandle {
public:
    AutoCloseHandle(const HANDLE handle): handle(handle) {}
    ~AutoCloseHandle() {
        CloseHandle(handle);
    }
private:
    HANDLE handle;
};