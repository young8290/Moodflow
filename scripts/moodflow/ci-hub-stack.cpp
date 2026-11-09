// Failure-only diagnostics for the disposable CI Hub. No memory dumps or input.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <dbghelp.h>
#include <tlhelp32.h>
#include <psapi.h>
#include <algorithm>
#include <array>
#include <cstdlib>
#include <iostream>
#include <string>

struct Handle {
    HANDLE value;
    ~Handle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
struct Resume {
    HANDLE thread;
    ~Resume() { ResumeThread(thread); }
};

int wmain(int argc, wchar_t** argv) {
    static_assert(sizeof(void*) == 8, "x64 diagnostics only");
    wchar_t ci[16]{};
    wchar_t temporary[32768]{};
    if (argc != 2 || !GetEnvironmentVariableW(L"GITHUB_ACTIONS", ci, 16) ||
        std::wstring(ci) != L"true" ||
        !GetEnvironmentVariableW(L"RUNNER_TEMP", temporary, 32768)) return 90;
    const DWORD pid = wcstoul(argv[1], nullptr, 10);
    if (!pid) return 91;
    Handle process{OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid)};
    if (!process.value) return 92;
    wchar_t executable[32768]{};
    DWORD size = 32768;
    if (!QueryFullProcessImageNameW(process.value, 0, executable, &size)) return 93;
    const std::wstring path(executable);
    const std::wstring prefix = std::wstring(temporary) + L"\\Moodflow Hub UI ";
    const std::wstring suffix = L"\\bin\\moodflow_hub.exe";
    if (path.size() < prefix.size() + suffix.size() ||
        _wcsnicmp(path.c_str(), prefix.c_str(), prefix.size()) != 0 ||
        _wcsicmp(path.c_str()+path.size()-suffix.size(),suffix.c_str()) != 0) return 94;
    // Local/module symbols only: never download a symbol file or dump process memory.
    SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_UNDNAME | SYMOPT_FAIL_CRITICAL_ERRORS);
    if (!SymInitialize(process.value, ".", TRUE)) return 95;
    Handle snapshot{CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0)};
    THREADENTRY32 entry{};
    entry.dwSize = sizeof(entry);
    unsigned inspected = 0;
    if (Thread32First(snapshot.value, &entry)) do {
        if (entry.th32OwnerProcessID != pid || ++inspected > 96) continue;
        Handle thread{OpenThread(THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME | THREAD_QUERY_INFORMATION,FALSE,entry.th32ThreadID)};
        if (!thread.value || SuspendThread(thread.value) == DWORD(-1)) continue;
        Resume resume{thread.value};
        CONTEXT context{};
        context.ContextFlags = CONTEXT_FULL;
        if (!GetThreadContext(thread.value,&context)) continue;
        std::wcout << L"thread=" << entry.th32ThreadID << L'\n';
        STACKFRAME64 frame{};
        frame.AddrPC.Offset = context.Rip;
        frame.AddrPC.Mode = AddrModeFlat;
        frame.AddrStack.Offset = context.Rsp;
        frame.AddrStack.Mode = AddrModeFlat;
        frame.AddrFrame.Offset = context.Rbp;
        frame.AddrFrame.Mode = AddrModeFlat;
        for (unsigned depth=0;depth<48 && frame.AddrPC.Offset;++depth) {
            const auto address=frame.AddrPC.Offset;
            const auto base=SymGetModuleBase64(process.value,address);
            wchar_t module[MAX_PATH]{};
            GetModuleBaseNameW(process.value,reinterpret_cast<HMODULE>(base),module,MAX_PATH);
            std::wcout << L"  " << module << L"+0x" << std::hex << (address-base) << std::dec;
            alignas(SYMBOL_INFO) std::array<char,sizeof(SYMBOL_INFO)+MAX_SYM_NAME> storage{};
            auto symbol=reinterpret_cast<SYMBOL_INFO*>(storage.data());
            symbol->SizeOfStruct=sizeof(SYMBOL_INFO);
            symbol->MaxNameLen=MAX_SYM_NAME;
            DWORD64 displacement=0;
            if (SymFromAddr(process.value,address,&displacement,symbol)) {
                const std::string name(symbol->Name,symbol->NameLen);
                std::wcout << L" " << std::wstring(name.begin(),name.end());
            }
            std::wcout << L'\n';
            if (!StackWalk64(IMAGE_FILE_MACHINE_AMD64,process.value,thread.value,&frame,&context,nullptr,SymFunctionTableAccess64,SymGetModuleBase64,nullptr)) break;
        }
    } while (Thread32Next(snapshot.value,&entry));
    SymCleanup(process.value);
    return 0;
}
