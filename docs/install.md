# Install Lenso CLI

Lenso is a native executable with its Wasm runtime linked into the binary. Installing
and running the CLI does not require Node.js, npm, Git, gh, Rust or Python. The
installer uses operating-system tools; Python is only used by the release test driver.

## Release assets

Download from the [CLI Releases](https://github.com/LioRael/lenso-cli/releases/tag/v0.1.0).
CLI tags use `v<VERSION>`; extension tags such as `code-v0.1.1` distribute separate
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

For a public repository, download the pinned installer before running it:

```sh
curl -fsSL https://github.com/LioRael/lenso-cli/releases/download/v0.1.0/install.sh -o install.sh
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
lenso --version
```

For the latest stable CLI installer, replace `download/v0.1.0` with `latest/download`.
Each downloaded installer is pinned to its own exact Release, even if `latest`
changes during installation. Requirements are `sh`, `curl`, `tar`, standard file
utilities and `sha256sum` (Linux) or `shasum` (macOS). It does not need jq or a JSON parser.

The default destination is `$HOME/.local/bin`. Override it with
`--prefix /absolute/directory` or `LENSO_INSTALL_DIR`. `--force` explicitly permits
replacing an existing regular executable. Links/directories are refused. The
installer checks the archive SHA-256 and executable version before replacing the
destination with a file staged in that same directory. Failed downloads/checks
leave an existing installation intact. Shell startup files are not edited.

## Windows

Download `install.ps1` from the Release page, then run it in PowerShell:

```powershell
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

## Private repository access

The repository is currently private. Browser downloads require an account with
repository access. For scripted installation, set `GH_TOKEN` or `GITHUB_TOKEN` to
a credential allowed to read repository contents. Do not put the token in a URL.

The Release body includes a Unix bootstrap command with the actual `install.sh`
GitHub asset API URL, so private installation needs no gh or JSON parser. Download
the installer using that command, inspect it, then run `sh install.sh`. On Windows,
download the PowerShell installer from the authenticated Release page, then run it
with the token environment variable set. Both generated installers use pinned
asset API IDs and built-in archive hashes. Authorization is sent to GitHub's API;
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
tar -xzf lenso-v0.1.0-aarch64-apple-darwin.tar.gz
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
