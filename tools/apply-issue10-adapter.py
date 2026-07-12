from pathlib import Path

path = Path("adapters/toolos-winget-adapter/src/main.rs")
source = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    source = source.replace(old, new, 1)


replace_once(
    "use toolos_winget::{\n",
    "mod verification;\n\nuse toolos_winget::{\n",
    "verification module",
)
replace_once(
    '''        "winget.installed" => installed_request(request.params.clone()).await,
        "winget.install.execute" => execute_install_request(request.params.clone()).await,
''',
    '''        "winget.installed" => installed_request(request.params.clone()).await,
        "winget.verify" => verification::verify_request(request.params.clone()).await,
        "winget.install.execute" => execute_install_request(request.params.clone()).await,
''',
    "verification route",
)
replace_once(
    '''                "package.installed.query.winget",
                "package.preview.install",
''',
    '''                "package.installed.query.winget",
                "package.verify.winget",
                "package.preview.install",
''',
    "verification capability",
)
path.write_text(source, encoding="utf-8", newline="\n")

verification_path = Path("adapters/toolos-winget-adapter/src/verification.rs")
verification = verification_path.read_text(encoding="utf-8")
old = "    ProcessEvidence, ScopeRoots, WingetVerificationReport, OFFICIAL_PROVIDER_CONTRACT,\n"
new = "    ProcessEvidence, ScopeRoots, OFFICIAL_PROVIDER_CONTRACT,\n"
if verification.count(old) != 1:
    raise RuntimeError(
        f"unused verification import: expected one match, found {verification.count(old)}"
    )
verification_path.write_text(
    verification.replace(old, new, 1), encoding="utf-8", newline="\n"
)
