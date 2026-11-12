# Live GitHub Release verification

Executed on 2026-11-13 (Asia/Shanghai) in the private
[`LioRael/lenso-cli-distribution-e2e`](https://github.com/LioRael/lenso-cli-distribution-e2e)
repository. These are real GitHub Releases and a GitHub-hosted Actions run, separate
from the local REST fixture tests. Repository access is required to open the links.

## Published packages

| Publisher | Release | Source commit | Result |
| --- | --- | --- | --- |
| Native CLI on macOS | [code-v0.1.0](https://github.com/LioRael/lenso-cli-distribution-e2e/releases/tag/code-v0.1.0), ID `443567332` | `1dadefb8030ec5b51ec1569f956f410c7bb1e515` | Three verified assets uploaded, then published; identical repeat uploaded zero assets |
| Native CLI in Actions on Ubuntu | [code-v0.1.1](https://github.com/LioRael/lenso-cli-distribution-e2e/releases/tag/code-v0.1.1), ID `443575605` | `cc2cebc3c7438e668a2bb55924805ef566c5b923` | Built CLI and Wasm from source, uploaded and verified all assets, then published |

The [Actions run](https://github.com/LioRael/lenso-cli-distribution-e2e/actions/runs/43317107999)
and [job](https://github.com/LioRael/lenso-cli-distribution-e2e/actions/runs/43317107999/job/131527887823)
completed successfully. The run used `workflow_dispatch` with `draft=false`, the
committed Cargo lockfiles and Rust 1.99.0. Publishing and verification used the
repository-scoped `GITHUB_TOKEN` with `contents: write`.

Each Release contains `extension.toml`, `lenso-release.json` and a versioned archive.
The author CLI packaged prebuilt modules, created a draft, uploaded through GitHub's
asset API, checked SHA-256 digests and made the verified Release available to consumers.
The two publishers used distinct package versions. The verification repository's
`0.1.1` manifest is a test version; the local workspace preset remains `0.1.0`.

## Consumer verification

The macOS-published `0.1.0` package was consumed on macOS. The Actions-published `0.1.1`
package was consumed both inside the Ubuntu workflow and independently on macOS.
Every consumer started with a new project and empty cache. Each native Lenso process
ran with an empty PATH, excluding Node.js, npm, Git, gh, Rust and Python subprocesses.
Python only orchestrated the test outside the native consumer.

All three consumers passed:

- Cold Release discovery, asset downloads, module integrity checks and Wasm execution.
- Catalog search and source reads from `fixtures/distribution` at the exact source commit.
- Preview without application writes, a digest-bound question with exit 2, declined
  approval with exit 130 and no writes, then approved application with exact file content.
- A clean diff with unchanged `lenso.lock`.
- Complete cache removal followed by `--locked` restoration from immutable identities.
- `--locked --offline` diff and add with unreachable proxies and an unchanged lock.
- Structured JSON results without terminal escape codes or exposed credentials.

GitHub's advertised asset digests, the Ubuntu consumer and the independent macOS
consumer agreed on the `0.1.1` payloads:

| Payload | SHA-256 |
| --- | --- |
| Release metadata | `fc9ed8e9126df15727336a205b306ea42a6d6bf991d10fa7b5cbc994d6dcb402` |
| Manifest | `d38b0210ec49ca2218dcd61577df85bbc05dbe61f22da621afc3868eddb81402` |
| Archive | `e8ea70ec9d3734fab8a14092c730318680d9a13df31e2d1ffd6cabf0659b44a5` |
| Extracted Wasm | `008e338f35b0d07ee9c65c22f1016294a9e2eab08d300d265465800cf08db3b0` |

The reusable [verification script](../scripts/verify-github-release.py) retains
`verification.json` and per-command JSON traces in its output directory. Local evidence
is under `dist/github-live/`: `local-consumption`, `actions-consumption` and `evidence`.
The Actions log and step summary also record its successful consumer checks.

An additional tag-push run for the locally published `0.1.0` package was intentionally
cancelled while queued, avoiding a second build of that already published version.
The completed Actions verification above used the distinct `0.1.1` version.

## Repeat consumption

Set `GH_TOKEN` to a credential with access to the private repository, then use a new
output directory:

```sh
python3 scripts/verify-github-release.py --binary target/debug/lenso \
  --repo LioRael/lenso-cli-distribution-e2e --version 0.1.1 \
  --ref cc2cebc3c7438e668a2bb55924805ef566c5b923 \
  --token-env GH_TOKEN --out dist/repeat-live-consumer
```

These checks establish native publishing and portable Wasm consumption on the two
tested hosts. They do not establish a Windows run or a new release of the CLI binary.
