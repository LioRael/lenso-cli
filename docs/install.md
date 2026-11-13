# Install Lenso CLI

Lenso is a native executable with its Wasm runtime linked into the binary. Installing
and running the CLI does not require Node.js, npm, Git, gh, Rust or Python. The
installer uses operating-system tools; Python is only used by the release test driver.

## Release assets

Download from the [latest CLI Release](https://github.com/LioRael/lenso-cli/releases/latest).
CLI tags use `v<VERSION>`; extension tags such as `code-v0.1.2` distribute separate
packages. Native stable CLI Releases own the GitHub `latest` entry. Extension
publication explicitly leaves that entry unchanged.

| Platform | Target | Archive | Build/test host |
| --- | --- | --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` | macOS 15 ARM64 |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` | macOS 15 Intel |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.tar.gz` | Ubuntu 22.04 x64 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `.tar.gz` | Ubuntu 22.04 ARM64 |
| Windows x64 | `x86_64-pc-windows-msvc` | `.zip` | Windows Server 2022 x64 |

macOS builds use deployment target 13.0. Linux GNU builds require glibc 2.35 or
newer and the standard system libraries listed in `release.json`; these are not
Alpine/musl binaries. Windows builds statically link the C runtime and packaging
rejects imports from the Visual C++ Redistributable. Windows
ARM64 and 32-bit targets are not built. Releases do not have Developer ID or
Authenticode signatures, and macOS packages are not notarized.

Each archive contains `lenso` or `lenso.exe`, `release.json` with the target/version,
binary SHA-256 and system-library dependencies, and `README.txt`. `SHA256SUMS` covers
all five archives plus `install.sh` and `install.ps1`.

## macOS and Linux

Download the latest stable installer before running it:

```sh
curl -fsSL https://github.com/LioRael/lenso-cli/releases/latest/download/install.sh -o install.sh
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
lenso --version
```

To pin CLI 1.0.0, replace `latest/download` with `download/v1.0.0`.
Each downloaded installer is pinned to its own exact Release, even if `latest`
changes during installation. Requirements are `sh`, `curl`, `tar`, `gzip`, standard file
utilities and `sha256sum` (Linux) or `shasum` (macOS). It does not need jq or a JSON parser.

The default destination is `$HOME/.local/bin`. Override it with
`--prefix /absolute/directory` or `LENSO_INSTALL_DIR`. `--force` explicitly permits
replacing an existing regular executable. Links/directories are refused. The
installer checks the archive SHA-256 and executable version before replacing the
destination with a file staged in that same directory. Failed downloads/checks
leave an existing installation intact. Shell startup files are not edited.

## Windows

Download and run the latest stable installer in PowerShell:

```powershell
Invoke-WebRequest https://github.com/LioRael/lenso-cli/releases/latest/download/install.ps1 -OutFile install.ps1
& .\install.ps1
$env:PATH = "$env:LOCALAPPDATA\Lenso\bin;$env:PATH"
lenso --version
```

Use `-InstallDir 'C:\path\to\bin'` to choose another directory and `-Force` to
replace an existing regular executable. The default is `%LOCALAPPDATA%\Lenso\bin`.
The installer uses PowerShell/.NET for HTTPS, SHA-256, extraction and replacement.
It requires no curl, archive utility or development tool. Persistent PATH and
PowerShell execution policy are not changed. If local policy disallows scripts,
use the manual ZIP installation below.

## Upgrade the CLI

Native installations support explicit self-updates:

```sh
lenso self update --check
lenso self update
lenso self update --version 1.2.3
```

CLI 1.0.0 predates this command: first download the latest installer and rerun it
with `--force` (Unix) or `-Force` (Windows). Subsequent native upgrades use the
commands above.

The default channel is the official CLI's latest stable GitHub Release. Anonymous
updates resolve the public release redirect and download exact-version assets
directly from GitHub, without consuming the anonymous API quota. Credentialed
updates use release and asset API IDs. CLI 1.1.0 used the API for both paths; if
its anonymous quota is exhausted, provide a token or rerun the latest installer
to upgrade to 1.1.1 or newer. An exact
`--version` permits prereleases and downgrades. `lenso update` continues to update
project extension pins; it does not update the executable.

The installer writes `lenso-install.json` beside the executable, recording the
native installation method, version, target, executable name and SHA-256. Self-update
validates that receipt against the running CLI. Older native installations and
manually copied binaries have no receipt: use `lenso self update --adopt` to explicitly
allow native ownership. Adoption downloads and verifies a release even when the
selected version is already installed. A mismatched/corrupt receipt requires
reinstalling with the official installer. Keep the receipt with the binary when moving it.

Recognized Cargo and Homebrew locations are left to their package managers, including
when `--adopt` is supplied. Custom Cargo roots are recognized through Cargo's install
metadata. Unknown package-manager locations cannot always be identified; only use
`--adopt` for executables you intend to manage through the native updater.

Updates select the current build target, use exact-version URLs or authenticated asset IDs, and
check archive size/SHA-256, archive contents, package identity, binary SHA-256 and the
executable's reported version before replacement. GitHub asset digests are checked
when supplied. Downloads and verification failures preserve the existing executable
and receipt. Replacement uses a file staged in the installation directory; Windows
uses `self-replace` to replace the running executable and clean up the old image.
Concurrent self-updates are excluded through an operating-system file lock. The
executable and receipt are separate files: interruption between their replacements
can require reinstalling to repair the receipt.

`--check` performs no installation writes. `--offline` prevents release requests.
`--json` returns the normal command report, including the selected version and
executable path. Automatic checks never replace the executable.
HTTP errors distinguish exhausted GitHub API quotas from missing versions and
credential/access failures, without printing credentials or signed download URLs.
The updater needs write access to the installation directory and never elevates
privileges. It uses native HTTPS and archive handling, without shell installers or
external tools. Private access uses `GH_TOKEN`, then `GITHUB_TOKEN`, just like the
installer; credentials are stripped from redirects to a different origin.

Checksums provide integrity against release metadata, not an independent publisher
signature. Releases remain unsigned as described above.

### Version reminders

Successful interactive commands can show a newer-version reminder on stderr, using
only a local cached result. The reminder adds no filesystem or network work before
command startup. A missing or expired result schedules a detached worker after the command
finishes; it queries the public GitHub release redirect with a three-second timeout.
The first check's result becomes available to subsequent commands. The worker never
downloads or installs an executable and does not use GitHub API credentials.

Successful checks are cached for 24 hours; failed/interrupted checks back off for
one hour. An operating-system lock prevents concurrent workers from repeating a
request. Results live at `v1/update-notice/result.json` under `lenso cache path`,
including when `LENSO_CACHE_DIR` overrides that directory. Cache and network errors
are silent and never change the command's exit status.

CI, non-interactive use, `--offline`, `--json`, `--no-interactive`, help/version output
and `self` commands suppress both reminders and background checks. Set
`LENSO_NO_UPDATE_CHECK=1` to disable them for all commands. Cargo and Homebrew users
receive their package manager's upgrade instructions.

## Private repository access

For private repositories, browser downloads require an account with
repository access. For scripted installation, set `GH_TOKEN` or `GITHUB_TOKEN` to
a credential allowed to read repository contents. Do not put the token in a URL.

The Release body includes a Unix bootstrap command with the actual `install.sh`
GitHub asset API URL, so private installation needs no gh or JSON parser. Download
the installer using that command, inspect it, then run `sh install.sh`. On Windows,
download the PowerShell installer from the authenticated Release page, then run it
with the token environment variable set. Authenticated downloads use pinned
asset API IDs; anonymous downloads use exact-version Release URLs. Both paths
verify built-in archive hashes. Authorization is sent to GitHub's API;
download redirects clear it before contacting a different origin.

For public repositories no token is needed. An expiring signed CDN URL is never
stored in an installer. Repository renames require regenerating future installers;
existing scripts are pinned to the repository name and asset IDs at publication.

## Manual or offline installation

Download the archive for your architecture and `SHA256SUMS` using an authenticated
browser if needed. Verify the relevant archive line before extraction:

```sh
sha256sum --check --ignore-missing SHA256SUMS  # Linux
shasum -a 256 --check --ignore-missing SHA256SUMS  # macOS
tar -xzf lenso-v1.0.0-aarch64-apple-darwin.tar.gz
mkdir -p "$HOME/.local/bin"
cp lenso "$HOME/.local/bin/lenso"
```

On Windows, compare `(Get-FileHash ARCHIVE.zip -Algorithm SHA256).Hash` with its
entry in `SHA256SUMS`, then use `Expand-Archive ARCHIVE.zip` and copy `lenso.exe` to
a directory on PATH. This method works without executing a PowerShell script file.

An already downloaded generated installer can also verify/install an offline
archive: `sh install.sh --archive ARCHIVE.tar.gz`, or
`& .\install.ps1 -Archive ARCHIVE.zip`. Its embedded checksum must match the file.

## Publish the CLI

The [native release workflow](../.github/workflows/release-cli.yml) uses five native
runners and Rust 1.99.0. Update `workspace.package.version` and the workspace lockfile,
commit, then push the matching `v<VERSION>` tag, or dispatch `Release native CLI`.
A manual dispatch keeps a draft by default; select `draft=false` to publish after
verification. The tag must match the workspace version. It never moves an existing tag.

The pipeline builds and checks the executable format, architecture, version and
system-library dependencies on each native host. It packages deterministic archives
from those inputs, uploads a draft, verifies GitHub asset digests, and generates
installers containing the actual archive asset IDs and SHA-256 values. Matching
draft assets can be reused; conflicting assets and published versions are not overwritten.
Mutation requests are not automatically retried. Failures retain the draft for review.

If builds completed but distribution/verification needs a retry, dispatch with
`reuse_run_id` set to that native release run. The workflow checks its repository,
workflow path and all five successful native build jobs, then reads the version and
source commit from that run. Artifact downloads verify GitHub's artifact digests;
release assembly rechecks every package hash. Only upload/install/publish stages rerun,
using the original source commit and binaries. Tags remain bound to that exact commit.
For older source commits containing workflow changes, pre-create the verified tag
with a suitably authorized maintainer before resuming. Existing tags are checked
through GitHub's refs/commits APIs and are never moved.

Every native runner then installs from the real GitHub draft using restricted PATH
(Unix system utilities only, or empty PATH for PowerShell). Tests reject both accidental
replacement and a tampered archive without altering the installed binary. The installed
CLI then consumes the existing `code` Wasm package and pinned source catalog with a fresh
cache, exercises preview and declined/approved writes, restores a deleted cache under
`--locked`, and replays offline. After installation, the CLI version and Wasm consumer
commands run with empty PATH.

Only after all five installer/consumer jobs pass does the final job publish. It checks
the draft's sealed asset IDs, sizes, states and hashes again, preventing publication
of assets changed after verification. Stable CLI releases explicitly become `latest`;
prereleases and extension packages do not. Actions uses repository-scoped credentials;
no personal token needs to be stored in repository secrets.

After publication, dispatch `Verify latest native install` with the expected version.
All five platforms download the public `latest/download` installer and install with
GitHub credentials removed. They repeat the overwrite/tampering checks and Wasm
consumer verification. Each job uploads its installation report as an artifact.
