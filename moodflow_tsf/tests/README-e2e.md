# Isolated TSF acceptance

`tsf_e2e_host` is an optional MSVC target, enabled by `MOODFLOW_TSF_E2E=ON`.
It is not included in the installed payload. Both the native host and the Rust
test require `GITHUB_ACTIONS=true`; the orchestration script additionally checks
the Windows runner and the verified payload directory before registering a DLL.

`scripts/moodflow/ci-tsf-e2e.ps1` copies the actual release DLL and the host into a
separate fixture directory, registers that DLL, then runs the ignored coordinator
test. The Rust coordinator opens the actual request and push pipes and loads the
built table dictionary. There is no fake key handler, fake range, clipboard write,
SendInput, or interaction with another user's application.

Each host uses two system Rich Edit text stores with TSF enabled. Keys are routed
through `ITfKeystrokeMgr`; the expected table result must reach the first control.
The Rust test then explicitly binds a real Buffer target, sends synthetic text,
and requires the actual edit receipt before accepting removal. Switching to the
second control in the same process must revoke binding; an unbound send must
retain the Buffer and leave the second control empty. Duplicate insertion fails.

The ordinary, AppContainer and LPAC clients run sequentially. Sandboxed clients
use newly created profiles and actual process attributes; the host checks both
AppContainer token flag and actual resource access: a protected synthetic file
grants AAP read access and no restricted-package grant. AC must read it; LPAC
must receive ACCESS_DENIED. This verifies the documented LPAC resource boundary
without relying on the unsupported class-specific Win32 LPAC query contract.
Clients request `lpacCom` and `registryRead`; LPAC additionally requests the
system `lpacAppExperience` capability for the interactive application profile.
These names are listed in the [Chromium system capability constants](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/sandbox/policy/win/lpac_capability.h).
The AAP resource denial and private settings-pipe denial remain mandatory.
Public fixture binaries receive read/execute grants; synthetic reports receive
low-integrity write access. Repository files, credentials, dictionary and broker
user data are outside that grant. Child processes are job-bound and profiles are
deleted when they exit. The script unregisters its DLL in `finally`.

The test never silently skips an unavailable desktop, missing dictionary, denied
capability, absent edit context or failed DLL activation. The ignored annotation
only prevents normal unit tests from registering a TSF. A successful compile is
not acceptance: inspect the isolated Actions run and its three token/phase
reports before marking W5 complete.
