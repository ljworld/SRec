# Security policy

## Supported versions

| Version | Supported |
|---|---|
| Latest release or pre-release (currently 1.0.0-alpha) | ✅ |
| Older versions, AV1 Island 0.x | ❌ please update |

## Reporting a vulnerability

**Please don't open a public issue for security problems.**

Report privately through GitHub: open the repository's **Security** tab, then **Report a vulnerability**. GitHub's private vulnerability reporting must be enabled in the repository settings. Include:

- what's affected (installer, `srec.exe`, bundled FFmpeg / whisper.cpp, build pipeline)
- steps to reproduce, or a proof of concept
- the impact you expect

You'll get an acknowledgement within **7 days**. We'll keep you updated, agree a disclosure date with you, and credit you in the release notes unless you prefer otherwise.

## Scope and design notes

- SRec has **no network code**. It doesn't download updates or send data. The only downloads happen at **build time** (`build.ps1` and CI), and every one is pinned to a fixed release and verified by SHA-256. The LGPL FFmpeg build is checked by its build configuration.
- Bundled third-party programs (FFmpeg, whisper.cpp) are run as separate processes with fixed arguments. Vulnerabilities in them should also be reported upstream. We'll update the bundled versions.
- Releases are built by GitHub Actions from the public source and published with `SHA256SUMS.txt`.
