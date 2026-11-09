# Real packaged Hub + core, synthetic data, disposable Windows CI only.
param([Parameter(Mandatory)][string]$Payload)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
    $env:GITHUB_REPOSITORY -cne 'young8290/Moodflow' -or -not $env:RUNNER_TEMP) {
    throw 'Packaged UI acceptance requires an isolated Windows runner.'
}
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$payloadDir=(Resolve-Path -LiteralPath $Payload).Path
if ($payloadDir.TrimEnd('\') -ine (Join-Path $root 'build').TrimEnd('\')) { throw 'Use the verified payload.' }
if (Get-Process moodflow_core,moodflow_hub -ErrorAction SilentlyContinue) { throw 'Fresh runner required.' }
$fixture=Join-Path $env:RUNNER_TEMP ('Moodflow Hub UI '+[guid]::NewGuid().ToString('N'))
$bin=Join-Path $fixture 'bin'
New-Item -ItemType Directory -Path $bin | Out-Null
# This copy contains only the verified distribution, never repository credentials.
Get-ChildItem -LiteralPath $payloadDir | Copy-Item -Destination $bin -Recurse
[IO.File]::WriteAllText((Join-Path $bin 'portable_mode'),'')
$user=Join-Path $bin 'localdata'
New-Item -ItemType Directory -Path $user | Out-Null
[IO.File]::WriteAllText((Join-Path $user 'config.toml'),"[moodflow]`nenabled = false`n")
foreach ($name in @('temp','webview')) {
    New-Item -ItemType Directory -Path (Join-Path $fixture $name) | Out-Null
}
$listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
$listener.Start()
$port=$listener.LocalEndpoint.Port
$listener.Stop()
function Start-Owned([string]$Name,[string[]]$Arguments) {
    $info=[Diagnostics.ProcessStartInfo]::new((Join-Path $bin $Name))
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    # The core is a background helper; Hub is the interactive UI under test.
    $info.WindowStyle=$(if ($Name -ceq 'moodflow_hub.exe') { [Diagnostics.ProcessWindowStyle]::Normal } else { [Diagnostics.ProcessWindowStyle]::Hidden })
    $info.WorkingDirectory=$bin
    $info.Environment.Clear()
    foreach ($key in @('SystemRoot','WINDIR','PATH','ProgramFiles','ProgramFiles(x86)','ProgramData','CommonProgramFiles','CommonProgramFiles(x86)','USERPROFILE','LOCALAPPDATA','APPDATA')) {
        if ([Environment]::GetEnvironmentVariable($key)) { $info.Environment[$key]=[Environment]::GetEnvironmentVariable($key) }
    }
    # WebView2 uses the real Windows profile/known-folder identity. Business data,
    # browser data and temporary files are still explicitly isolated below.
    $info.Environment['TEMP']=Join-Path $fixture 'temp'
    $info.Environment['TMP']=Join-Path $fixture 'temp'
    $info.Environment['MF_HUB_DATA_DIR']=Join-Path $user 'hub'
    $info.Environment['WEBVIEW2_USER_DATA_FOLDER']=Join-Path $fixture 'webview'
    $info.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']="--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    return [Diagnostics.Process]::Start($info)
}
$core=$null
$hub=$null
New-Item -ItemType Directory -Force -Path (Join-Path $root 'dist') | Out-Null
try {
    $core=Start-Owned 'moodflow_core.exe' @()
    $hub=Start-Owned 'moodflow_hub.exe' @('--open','dashboard')
    & node (Join-Path $PSScriptRoot 'ci-hub-ui.mjs') $port (Join-Path $root 'dist/hub-ui-smoke.json')
    if ($LASTEXITCODE -ne 0) { throw 'Packaged Hub UI/native acceptance failed; see bounded synthetic report.' }
} finally {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class MoodflowWindowProbe {
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsHungAppWindow(IntPtr window);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("advapi32.dll", SetLastError=true)]
    static extern IntPtr OpenThreadWaitChainSession(uint flags, IntPtr callback);
    [DllImport("advapi32.dll")]
    static extern void CloseThreadWaitChainSession(IntPtr session);
    [DllImport("advapi32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetThreadWaitChain(IntPtr session, UIntPtr context, uint flags, uint thread, ref uint count, IntPtr nodes, out int cycle);
    [DllImport("advapi32.dll")]
    static extern void RegisterWaitChainCOMCallback(IntPtr call, IntPtr activation);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
    static extern IntPtr LoadLibraryEx(string name, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Ansi, ExactSpelling=true)]
    static extern IntPtr GetProcAddress(IntPtr module, string name);
    public static string[] Waits(Process process) {
        // Read only the owned synthetic process. Never collect lock names or memory.
        var lines = new List<string>();
        uint owner;
        uint ui = GetWindowThreadProcessId(process.MainWindowHandle, out owner);
        lines.Add("uiThread=" + ui);
        IntPtr ole = LoadLibraryEx("ole32.dll", IntPtr.Zero, 0x800);
        if (ole != IntPtr.Zero) RegisterWaitChainCOMCallback(GetProcAddress(ole,"CoGetCallState"), GetProcAddress(ole,"CoGetActivationState"));
        IntPtr session = OpenThreadWaitChainSession(0,IntPtr.Zero);
        if (session == IntPtr.Zero) return new [] { "wctError=" + Marshal.GetLastWin32Error() };
        // WAITCHAIN_NODE_INFO is 280 bytes on x64 (wct.h). Only fixed metadata is read.
        IntPtr nodes = Marshal.AllocHGlobal(280*16);
        try {
            int inspected = 0;
            foreach (ProcessThread thread in process.Threads) {
                if (++inspected > 96) break;
                uint count=16;
                int cycle;
                if (!GetThreadWaitChain(session,UIntPtr.Zero,0,(uint)thread.Id,ref count,nodes,out cycle)) {
                    lines.Add("thread="+thread.Id+" error="+Marshal.GetLastWin32Error());
                    continue;
                }
                string line="thread="+thread.Id+" cycle="+cycle;
                for (int i=0;i<Math.Min(count,16);i++) {
                    IntPtr node=IntPtr.Add(nodes,i*280);
                    int type=Marshal.ReadInt32(node,0);
                    line += " type="+type+" status="+Marshal.ReadInt32(node,4);
                    if (type==8) line += " pid="+Marshal.ReadInt32(node,8)+" tid="+Marshal.ReadInt32(node,12);
                }
                lines.Add(line);
            }
        } finally { Marshal.FreeHGlobal(nodes); CloseThreadWaitChainSession(session); }
        return lines.ToArray();
    }
}
'@
    if ($null -ne $hub -and -not $hub.HasExited -and [MoodflowWindowProbe]::IsHungAppWindow($hub.MainWindowHandle)) {
        [MoodflowWindowProbe]::Waits($hub) | Set-Content -LiteralPath (Join-Path $root 'dist/hub-ui-waits.txt') -Encoding utf8
        try {
            $diagnostics=Join-Path $fixture 'diagnostics'
            New-Item -ItemType Directory -Path $diagnostics | Out-Null
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'ci-hub-stack.cpp') -Destination $diagnostics
            @'
cmake_minimum_required(VERSION 3.20)
project(HubDiagnostics LANGUAGES CXX)
add_executable(hub_stack ci-hub-stack.cpp)
target_compile_features(hub_stack PRIVATE cxx_std_17)
target_compile_options(hub_stack PRIVATE /W4 /WX /utf-8)
target_link_libraries(hub_stack PRIVATE dbghelp psapi)
'@ | Set-Content -LiteralPath (Join-Path $diagnostics 'CMakeLists.txt') -Encoding utf8
            & cmake -S $diagnostics -B (Join-Path $diagnostics 'build') -G 'Visual Studio 17 2022' -A x64
            if ($LASTEXITCODE -ne 0) { throw 'Configure diagnostics failed.' }
            & cmake --build (Join-Path $diagnostics 'build') --config Release --parallel 1
            if ($LASTEXITCODE -ne 0) { throw 'Compile diagnostics failed.' }
            $probe=Start-Process -FilePath (Join-Path $diagnostics 'build/Release/hub_stack.exe') -ArgumentList $hub.Id -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root 'dist/hub-ui-stack.txt')
            try {
                if (-not $probe.WaitForExit(15000)) { $probe.Kill(); $probe.WaitForExit(5000) | Out-Null }
            } finally { $probe.Dispose() }
        } catch { Write-Warning 'Unable to collect bounded native stack diagnostics.' }
    }
    [pscustomobject]@{
        coreExited=($null -eq $core -or $core.HasExited)
        hubExited=($null -eq $hub -or $hub.HasExited)
        hubWindowTitle=$(if ($null -ne $hub -and -not $hub.HasExited) { $hub.MainWindowTitle } else { '' })
        hubWindowHung=$(if ($null -ne $hub -and -not $hub.HasExited) { [MoodflowWindowProbe]::IsHungAppWindow($hub.MainWindowHandle) } else { $null })
        hubExitCode=$(if ($null -ne $hub -and $hub.HasExited) { $hub.ExitCode } else { $null })
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'dist/hub-ui-processes.json') -Encoding utf8
    foreach ($process in @($hub,$core)) {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit(10000) | Out-Null }
            } finally { $process.Dispose() }
        }
    }
    # Preserve only application diagnostics, never the synthetic vault/profile.
    $logs=Join-Path $user 'logs'
    if (Test-Path -LiteralPath $logs) {
        Copy-Item -LiteralPath $logs -Destination (Join-Path $root 'dist/hub-ui-core-logs') -Recurse -Force
    }
    $logs=Join-Path $user 'hub/logs'
    if (Test-Path -LiteralPath $logs) {
        Copy-Item -LiteralPath $logs -Destination (Join-Path $root 'dist/hub-ui-hub-logs') -Recurse -Force
    }
}
