# Locale-Stable WinGet Verification Design

## Goal

Separate provider execution success, installed-package identity, and application health without parsing localized WinGet table output or inventing unsupported identity claims.

## Official source ledger

The supported package-presence provider is the official `Microsoft.WinGet.Client` PowerShell module at Microsoft `winget-cli` source commit `22d5c7d891a30f9d1b52214ba4a6bdbb14183fb1`.

ToolOS uses only these copied contracts:

- `Get-WinGetPackage` emits `PSInstalledCatalogPackage` objects.
- `FinderCmdlet` accepts `Id`, `Source`, and `MatchOption`.
- `PSPackageFieldMatchOption` contains `Equals` and `EqualsCaseInsensitive`.
- `PSCatalogPackage` exposes `Id` and `Source`.
- `PSInstalledCatalogPackage` exposes `InstalledVersion` and `CompareToVersion(string)`.

ToolOS does not use localized formatted tables. A constant PowerShell script serializes only the copied properties to ToolOS-owned JSON contract `toolos.microsoft-winget-client.installed/1`.

The official object does not expose installed scope or installed architecture. Those dimensions must be proven by a package recipe or remain `INDETERMINATE`.

## Three independent verdicts

1. **Provider result**: Did the governed WinGet process complete, time out, fail, or become recovery-unknown?
2. **Installed-package identity**: Does the official typed provider report exactly one matching ID/source/version, and can required scope/architecture be proven?
3. **Application health**: Does a package-specific recipe prove the expected executable or service works?

No verdict implies another.

## Verdict vocabulary

Every verification dimension returns exactly one of:

- `VERIFIED`
- `NOT_VERIFIED`
- `INDETERMINATE`

`NOT_VERIFIED` requires contradictory evidence, such as no exact package, wrong version, wrong architecture, wrong scope, missing executable, or failing health probe. `INDETERMINATE` means the current provider/recipe cannot prove the dimension.

Overall package identity is `VERIFIED` only when ID, source, version, scope, and architecture are all `VERIFIED`. One `NOT_VERIFIED` makes the package verdict `NOT_VERIFIED`; otherwise it is `INDETERMINATE`.

Application health is independently `HEALTHY`, `UNHEALTHY`, or `INDETERMINATE`.

## Official provider probe

The adapter launches `powershell.exe` without a shell, profile, or interactive input. Selector data is passed as base64-encoded UTF-8 JSON in one environment variable. The constant script:

1. finds the highest available `Microsoft.WinGet.Client` module;
2. imports it with `-ErrorAction Stop`;
3. calls `Get-WinGetPackage -Id <id> -Source <source> -MatchOption Equals`;
4. serializes module version and each object's `Id`, `Source`, `InstalledVersion`, and `CompareToVersion(requestedVersion)` result;
5. writes one compressed JSON object to stdout.

If the module or copied properties are unavailable, the provider contract is unsupported and package identity is `INDETERMINATE`; ToolOS does not fall back to localized text parsing.

## Package recipes

Recipes are static, reviewed data. They cannot contain arbitrary commands. Version parsers and health evaluators are named built-ins implemented in Rust.

The first recipe is `Git.Git`:

- executable candidates: `git.exe`, `git`;
- health command: `git --version`;
- parser: `GIT_VERSION_V1`, accepting the documented output shape `git version <numeric-version>` with optional `.windows.<n>` suffix;
- architecture evidence: read the resolved Windows PE `Machine` field (`0x014c` x86, `0x8664` x64, `0xaa64` arm64);
- scope evidence: classify a canonical executable path only when it is below an explicit user root (`LOCALAPPDATA`, `APPDATA`, `USERPROFILE`) or system root (`ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`, `SystemRoot`); otherwise return `INDETERMINATE`.

The recipe never searches arbitrary directories, executes package scripts, or accepts regex/config from users.

## Windows CI evidence

The permanent Windows job exercises the real `Git.Git` recipe against the runner's installed Git executable. That test proves executable discovery, real `git --version` parsing, and PE architecture inspection. It records actual scope as verified or indeterminate based on the runner path; it does not require the runner to match ToolOS's user-scope install policy.

Fixture tests prove:

- exact provider JSON parses for the supported contract;
- wrong ID/source/version is `NOT_VERIFIED`;
- duplicate exact matches are `INDETERMINATE`;
- absent module/unsupported contract is `INDETERMINATE`;
- wrong PE architecture is `NOT_VERIFIED`;
- wrong classified scope is `NOT_VERIFIED`;
- missing executable is `NOT_VERIFIED`;
- provider package identity can be verified while the healthcheck is unhealthy.

## Execution integration

After a contained provider execution and post-state query, ToolOS runs `winget.verify` with the immutable selector. The verification report is persisted as independent evidence and embedded in the execution report.

Execution plan status becomes:

- `EXECUTION_VERIFIED_HEALTHY` only when provider success, package identity `VERIFIED`, and application health `HEALTHY` all hold;
- `EXECUTION_SUCCEEDED_UNVERIFIED` when provider success is known but identity or health is `INDETERMINATE`;
- `EXECUTION_VERIFICATION_FAILED` when package identity is `NOT_VERIFIED` or health is `UNHEALTHY`;
- existing timeout/cancel/failure/recovery statuses remain unchanged.

## Safety boundary

Verification is read-only. It may invoke only the official typed provider probe and named recipe health commands. It cannot install modules, mutate sources, accept agreements, repair packages, update packages, uninstall packages, or execute user-supplied commands.