// Isolated Windows acceptance host. All text is synthetic; no SendInput, clipboard or
// user application is used. The actual registered DLL edits two real Rich Edit stores.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#define _RICHEDIT_VER 0x0800
#include <windows.h>
#include <msctf.h>
#include <richedit.h>
#include <userenv.h>
#include <sddl.h>
#include <wrl/client.h>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <algorithm>

using Microsoft::WRL::ComPtr;
namespace fs = std::filesystem;
static const std::wstring kBuffer = L" buffered";
static void Check(bool good, const char* stage) { if (!good) throw std::runtime_error(stage); }
static void Hr(HRESULT result, const char* stage) {
    if (FAILED(result)) { std::ostringstream message; message << stage << " HRESULT=" << std::hex << static_cast<unsigned long>(result); throw std::runtime_error(message.str()); }
}
static void Pump(DWORD milliseconds) {
    const auto start = GetTickCount64();
    do {
        MSG message{};
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
        Sleep(5);
    } while (GetTickCount64() - start < milliseconds);
}
template<class F> static void Until(F predicate, const char* stage, DWORD timeout = 20000) {
    const auto start = GetTickCount64();
    while (!predicate()) { Check(GetTickCount64() - start < timeout, stage); Pump(10); }
}
static void Mark(const fs::path& report, const char* name, const std::string& text = "ok") {
    std::ofstream file(report / name, std::ios::binary | std::ios::trunc); file << text; file.close(); Check(!file.fail(), "write report");
}
static std::wstring Text(HWND window) {
    std::wstring text(static_cast<size_t>(GetWindowTextLengthW(window)) + 1, L'\0');
    const auto length = GetWindowTextW(window, text.data(), static_cast<int>(text.size()));
    text.resize(static_cast<size_t>(length)); return text;
}
static LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    return DefWindowProcW(window, message, wparam, lparam);
}
static DWORD TokenFlag(TOKEN_INFORMATION_CLASS type) {
    HANDLE token = nullptr; Check(OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token) != FALSE, "open own token");
    DWORD flag = 0, bytes = 0; const BOOL ok = GetTokenInformation(token, type, &flag, sizeof(flag), &bytes);
    const auto error = ok ? ERROR_SUCCESS : GetLastError(); CloseHandle(token);
    Check(ok != FALSE, ("query token class=" + std::to_string(type) + " error=" + std::to_string(error)).c_str()); return flag;
}
static std::wstring Env(const wchar_t* key) {
    const auto size=GetEnvironmentVariableW(key,nullptr,0); if (!size) return {};
    std::wstring value(size,L'\0'); const auto got=GetEnvironmentVariableW(key,value.data(),size);
    Check(got>0 && got<size,"read fixture environment"); value.resize(got); return value;
}
static void CheckPrivateRpc(const std::wstring& kind,const fs::path& report) {
    for (const auto name : {L"MOODFLOW_TSF_E2E_RPC_CTRL",L"MOODFLOW_TSF_E2E_RPC_EVENTS"}) {
        const auto endpoint=Env(name); Check(!endpoint.empty(),"private RPC fixture endpoint missing");
        HANDLE handle=INVALID_HANDLE_VALUE; DWORD error=0;
        for (int attempt=0;attempt<100;++attempt) {
            handle=CreateFileW(endpoint.c_str(),GENERIC_READ,0,nullptr,OPEN_EXISTING,0,nullptr);
            if (handle!=INVALID_HANDLE_VALUE) break;
            error=GetLastError(); if (error!=ERROR_PIPE_BUSY) break; Sleep(20);
        }
        if (kind==L"ordinary") { Check(handle!=INVALID_HANDLE_VALUE,"same-user medium RPC access denied"); CloseHandle(handle); }
        else { if (handle!=INVALID_HANDLE_VALUE) CloseHandle(handle); Check(handle==INVALID_HANDLE_VALUE && error==ERROR_ACCESS_DENIED,"sandbox could access settings RPC"); }
    }
    Mark(report,"private-rpc",kind==L"ordinary"?"owner allowed":"sandbox denied");
}
static bool CheckAppPackageAccess(const std::wstring& kind) {
    const auto path=Env(L"MOODFLOW_TSF_E2E_AAP_PROBE"); Check(!path.empty(),"missing AAP access probe");
    const auto file=CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,0,nullptr);
    const auto error=file==INVALID_HANDLE_VALUE?GetLastError():ERROR_SUCCESS;
    const bool readable=file!=INVALID_HANDLE_VALUE; if (readable) CloseHandle(file);
    // LPAC disregards ALL APPLICATION PACKAGES. The AC positive control must
    // read the same synthetic protected file; LPAC must get ACCESS_DENIED.
    Check(readable==(kind!=L"lpac") && (readable || error==ERROR_ACCESS_DENIED),
        ("AAP resource access mismatch error="+std::to_string(error)).c_str());
    return readable;
}
static void Key(ITfKeystrokeMgr* keys, UINT key) {
    const LPARAM scan = static_cast<LPARAM>(MapVirtualKeyW(key, MAPVK_VK_TO_VSC)) << 16;
    BOOL eaten = FALSE;
    Hr(keys->TestKeyDown(key, scan | 1, &eaten), "test key down"); Check(eaten != FALSE, "TSF did not claim fixture key");
    Hr(keys->KeyDown(key, scan | 1, &eaten), "real key down"); Check(eaten != FALSE, "TSF did not consume fixture key");
    Pump(50);
    BOOL up = FALSE; Hr(keys->TestKeyUp(key, scan | 1, &up), "test key up");
    if (up) Hr(keys->KeyUp(key, scan | static_cast<LPARAM>(0xc0000001u), &up), "real key up");
    Pump(30);
}
static int Client(const std::wstring& kind, const fs::path& report) {
    const auto app = TokenFlag(TokenIsAppContainer);
    const auto aap = CheckAppPackageAccess(kind);
    const auto lpac = app != 0 && !aap ? 1u : 0u;
    Check(app == (kind == L"ordinary" ? 0u : 1u), "wrong AppContainer token");
    Check(lpac == (kind == L"lpac" ? 1u : 0u), "wrong LPAC token");
    Mark(report, "token", std::to_string(app) + ":" + std::to_string(lpac));
    CheckPrivateRpc(kind,report);
    Hr(CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED), "COM STA");
    Check(LoadLibraryW(L"Msftedit.dll") != nullptr, "load system Rich Edit");
    {
        ComPtr<ITfThreadMgr> manager;
        Hr(CoCreateInstance(CLSID_TF_ThreadMgr, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&manager)), "create real TSF manager");
        TfClientId client = TF_CLIENTID_NULL; Hr(manager->Activate(&client), "activate TSF manager");
        WNDCLASSW cls{}; cls.lpfnWndProc = WindowProc; cls.hInstance = GetModuleHandleW(nullptr); cls.lpszClassName = L"Moodflow.TsfE2E";
        Check(RegisterClassW(&cls) != 0, "register fixture window");
        HWND window = CreateWindowExW(0, cls.lpszClassName, L"Moodflow isolated TSF acceptance", WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            80, 80, 560, 260, nullptr, nullptr, cls.hInstance, nullptr);
        Check(window != nullptr, "create fixture window");
        HWND first = CreateWindowExW(0, MSFTEDIT_CLASS, L"", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_MULTILINE,
            10, 10, 500, 70, window, nullptr, cls.hInstance, nullptr);
        HWND second = CreateWindowExW(0, MSFTEDIT_CLASS, L"", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_MULTILINE,
            10, 100, 500, 70, window, nullptr, cls.hInstance, nullptr);
        Check(first && second, "create two real text stores");
        SendMessageW(first, EM_SETEDITSTYLE, SES_USECTF, SES_USECTF);
        SendMessageW(second, EM_SETEDITSTYLE, SES_USECTF, SES_USECTF);
        SetForegroundWindow(window); SetActiveWindow(window); SetFocus(window); Pump(100);
        ComPtr<ITfInputProcessorProfiles> profiles;
        Hr(CoCreateInstance(CLSID_TF_InputProcessorProfiles, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&profiles)), "create TSF profiles");
        ComPtr<ITfInputProcessorProfileMgr> profileManager; Hr(profiles.As(&profileManager), "profile manager");
        CLSID clsid{}; GUID profile{};
        Hr(CLSIDFromString(L"{EF62EE30-5ECF-413A-A476-48D1F29E827C}", &clsid), "CLSID");
        Hr(CLSIDFromString(L"{EF62EE31-5ECF-413A-A476-48D1F29E827C}", &profile), "profile GUID");
        if (kind == L"ordinary") Hr(profiles->EnableLanguageProfile(clsid, 0x0804, profile, TRUE), "enable isolated user profile");
        Hr(profiles->ChangeCurrentLanguage(0x0804), "select fixture language");
        const auto active = profileManager->ActivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, 0x0804, clsid, profile, nullptr, 0);
        if (FAILED(active)) {
            // Diagnose the real failure; never substitute a directly activated TIP.
            wchar_t exe[32768]{}; GetModuleFileNameW(nullptr,exe,32768);
            const auto dll=fs::path(exe).parent_path()/L"moodflow_tsf.dll";
            const auto module=LoadLibraryW(dll.c_str()); const auto loadError=module?ERROR_SUCCESS:GetLastError();
            ComPtr<IUnknown> direct; const auto com=CoCreateInstance(clsid,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&direct));
            HKEY key=nullptr;
            const auto registry=RegOpenKeyExW(HKEY_LOCAL_MACHINE,L"Software\\Classes\\CLSID\\{EF62EE30-5ECF-413A-A476-48D1F29E827C}\\InprocServer32",0,KEY_READ,&key);
            if (key) RegCloseKey(key);
            std::ostringstream diagnostic; diagnostic<<"load_error="<<loadError<<" registry_error="<<registry<<" direct_com="<<std::hex<<static_cast<unsigned long>(com);
            Mark(report,"activation-diagnostic",diagnostic.str());
            direct.Reset(); if (module) FreeLibrary(module);
            Hr(active,("activate actual registered Moodflow DLL "+diagnostic.str()).c_str());
        }
        Hr(active, "activate actual registered Moodflow DLL"); Check(active == S_OK, "profile not enabled");
        // The profile must be active before the editor gains focus. Activation
        // alone only reports IME state; an actual focus transition establishes
        // the document revision used by the production Buffer authorization.
        SetFocus(first); SendMessageW(first, EM_SETCTFOPENSTATUS, TRUE, 0); Pump(1200);
        ComPtr<ITfDocumentMgr> document; Hr(manager->GetFocus(&document), "get focused document"); Check(document != nullptr, "Rich Edit has no TSF document");
        ComPtr<ITfContext> context; Hr(document->GetTop(&context), "get actual text context"); Check(context != nullptr, "Rich Edit has no text context");
        ComPtr<ITfKeystrokeMgr> keys; Hr(manager.As(&keys), "keystroke manager");
        // This goes through TSF -> the packaged DLL -> the real bridge and table engine.
        for (int index = 0; index < 4; ++index) Key(keys.Get(), L'G');
        if (Text(first) != L"王") Key(keys.Get(), VK_SPACE);
        Until([&] { return Text(first) == L"王"; }, "table engine did not commit expected fixture");
        Mark(report, "ready");
        Until([&] { return Text(first) == L"王" + kBuffer; }, "Buffer delivery did not reach real text store");
        Mark(report, "delivered");
        SetFocus(second); Pump(500); Mark(report, "switched");
        Until([&] { return fs::exists(report / "finish"); }, "coordinator did not finish stale-target check");
        Pump(300); Check(Text(second).empty(), "old Buffer text leaked to the new field");
        Check(Text(first) == L"王" + kBuffer, "first field changed or delivery duplicated");
        context.Reset(); document.Reset(); keys.Reset();
        DestroyWindow(window);
        Hr(profileManager->DeactivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, 0x0804, clsid, profile, nullptr, 0), "deactivate fixture profile");
        Hr(manager->Deactivate(), "deactivate fixture manager");
    }
    CoUninitialize(); Mark(report, "passed"); return 0;
}

struct CapabilityMemory {
    std::vector<PSID> allocations;
    std::vector<SID_AND_ATTRIBUTES> capabilities;
    ~CapabilityMemory() { for (auto value : allocations) LocalFree(value); }
    void Add(const wchar_t* name) {
        using Derive = BOOL (WINAPI*)(LPCWSTR, PSID**, DWORD*, PSID**, DWORD*);
        const auto derive = reinterpret_cast<Derive>(GetProcAddress(GetModuleHandleW(L"kernelbase.dll"), "DeriveCapabilitySidsFromName"));
        Check(derive != nullptr, "derive capability API");
        PSID* groups = nullptr; PSID* sids = nullptr; DWORD groupCount = 0, count = 0;
        Check(derive(name, &groups, &groupCount, &sids, &count) != FALSE, "derive required TSF capability");
        for (DWORD index = 0; index < groupCount; ++index) allocations.push_back(groups[index]);
        allocations.push_back(groups);
        for (DWORD index = 0; index < count; ++index) { allocations.push_back(sids[index]); capabilities.push_back({ sids[index], SE_GROUP_ENABLED }); }
        allocations.push_back(sids);
    }
};
static std::wstring Quote(const std::wstring& value) {
    Check(value.find(L'"') == std::wstring::npos, "unexpected quote in fixture path"); return L"\"" + value + L"\"";
}
static std::vector<wchar_t> ChildEnvironment(PSID package,const fs::path& bin) {
    wchar_t system[32768]{},windows[32768]{};
    Check(GetSystemDirectoryW(system,32768)>0 && GetWindowsDirectoryW(windows,32768)>0,"system fixture paths");
    LPWSTR sid=nullptr; Check(ConvertSidToStringSidW(package,&sid)!=FALSE,"package SID string");
    PWSTR folder=nullptr; const auto result=GetAppContainerFolderPath(sid,&folder); LocalFree(sid); Hr(result,"package private folder");
    const std::wstring local=folder; CoTaskMemFree(folder); const auto temp=local+L"\\Temp";
    Check(CreateDirectoryW(temp.c_str(),nullptr)!=FALSE || GetLastError()==ERROR_ALREADY_EXISTS,"package temp directory");
    std::vector<std::wstring> entries={L"GITHUB_ACTIONS=true",L"SystemRoot="+std::wstring(windows),L"WINDIR="+std::wstring(windows),
        L"PATH="+std::wstring(system)+L";"+windows+L";"+bin.wstring(),L"LOCALAPPDATA="+local,L"TEMP="+temp,L"TMP="+temp};
    for (const auto name : {L"MOODFLOW_TSF_E2E_RPC_CTRL",L"MOODFLOW_TSF_E2E_RPC_EVENTS",L"MOODFLOW_TSF_E2E_AAP_PROBE"}) entries.push_back(std::wstring(name)+L"="+Env(name));
    std::sort(entries.begin(),entries.end()); std::vector<wchar_t> block;
    for (const auto& entry:entries) { block.insert(block.end(),entry.begin(),entry.end());block.push_back(L'\0'); }
    block.push_back(L'\0'); return block;
}
static int Launch(const std::wstring& kind, const fs::path& report) {
    wchar_t exe[32768]{}; Check(GetModuleFileNameW(nullptr, exe, 32768) != 0, "own executable path");
    const auto moniker = L"Moodflow.TsfE2E." + std::to_wstring(GetCurrentProcessId()) + L"." + std::to_wstring(GetTickCount64());
    PSID package = nullptr;
    Hr(CreateAppContainerProfile(moniker.c_str(), L"Moodflow TSF acceptance", L"Isolated synthetic text test", nullptr, 0, &package), "create isolated profile");
    int exitCode = 1;
    try {
        CapabilityMemory memory; memory.Add(L"lpacCom"); memory.Add(L"registryRead");
        SECURITY_CAPABILITIES security{}; security.AppContainerSid = package;
        security.Capabilities = memory.capabilities.data(); security.CapabilityCount = static_cast<DWORD>(memory.capabilities.size());
        SIZE_T bytes = 0; const DWORD count = kind == L"lpac" ? 2u : 1u;
        InitializeProcThreadAttributeList(nullptr, count, 0, &bytes);
        std::vector<unsigned char> storage(bytes);
        auto attributes = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(storage.data());
        Check(InitializeProcThreadAttributeList(attributes, count, 0, &bytes) != FALSE, "initialize sandbox attributes");
        Check(UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &security, sizeof(security), nullptr, nullptr) != FALSE, "set AppContainer identity");
        DWORD policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
        if (kind == L"lpac") Check(UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY, &policy, sizeof(policy), nullptr, nullptr) != FALSE, "set LPAC policy");
        STARTUPINFOEXW startup{}; startup.StartupInfo.cb = sizeof(startup); startup.lpAttributeList = attributes;
        PROCESS_INFORMATION process{};
        std::wstring command = Quote(exe) + L" --child " + kind + L" " + Quote(report.wstring());
        auto environment=ChildEnvironment(package,fs::path(exe).parent_path());
        HANDLE job = CreateJobObjectW(nullptr, nullptr); Check(job != nullptr, "create fixture child job");
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{}; limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        Check(SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits, sizeof(limits)) != FALSE, "bound fixture lifetime");
        const BOOL created = CreateProcessW(exe, command.data(), nullptr, nullptr, FALSE,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT, environment.data(), fs::path(exe).parent_path().c_str(), &startup.StartupInfo, &process);
        DeleteProcThreadAttributeList(attributes); Check(created != FALSE, "launch actual sandbox token");
        if (!AssignProcessToJobObject(job, process.hProcess)) { TerminateProcess(process.hProcess, 1); CloseHandle(process.hThread); CloseHandle(process.hProcess); CloseHandle(job); throw std::runtime_error("assign sandbox job"); }
        ResumeThread(process.hThread); CloseHandle(process.hThread);
        if (WaitForSingleObject(process.hProcess, 90000) != WAIT_OBJECT_0) {
            TerminateJobObject(job, 1); WaitForSingleObject(process.hProcess, 5000);
        }
        DWORD code = 1; GetExitCodeProcess(process.hProcess, &code); CloseHandle(process.hProcess); CloseHandle(job);
        exitCode = static_cast<int>(code);
        if (exitCode != 0 && !fs::exists(report / "failure")) Mark(report, "failure", "sandbox host exit=" + std::to_string(exitCode));
    } catch (...) { FreeSid(package); DeleteAppContainerProfile(moniker.c_str()); throw; }
    FreeSid(package); Hr(DeleteAppContainerProfile(moniker.c_str()), "delete own isolated profile"); return exitCode;
}
int wmain(int argc, wchar_t** argv) {
    wchar_t ci[16]{}; GetEnvironmentVariableW(L"GITHUB_ACTIONS", ci, 16);
    if (std::wstring(ci) != L"true") return 90;
    if (argc < 3) return 91;
    const bool child = std::wstring(argv[1]) == L"--child";
    if (child && argc != 4) return 91;
    const std::wstring kind = argv[child ? 2 : 1]; const fs::path report = argv[child ? 3 : 2];
    if (kind != L"ordinary" && kind != L"appcontainer" && kind != L"lpac") return 91;
    try { return child || kind == L"ordinary" ? Client(kind, report) : Launch(kind, report); }
    catch (const std::exception& error) {
        try { Mark(report, "failure", error.what()); } catch (...) { }
        return 1;
    }
}
