<!--suppress HtmlDeprecatedAttribute -->
<table>
<tr>
<td align="right">
  <b>Support:</b>
</td>
<td>
  <a href="https://github.com/sponsors/becmer">GitHub Sponsors</a> •
  <a href="https://www.buymeacoffee.com/becmer">Buy Me a Coffee</a> •
  <a href="https://ko-fi.com/becmer">Ko-fi</a>
</td>
</tr>
<tr>
<td align="right">
  <b>Manifesto:</b>
</td>
<td>
  <a href="https://medium.com/@kamil.becmer/i-dont-have-lifetime-for-your-bullshit-cdaed08d0646">I don’t have lifetime for your bullshit</a>
</td>
</tr>
</table>

# Cross-platform daemon lifecycle primitives

<p>
  <a href="https://crates.io/crates/daemonbit"><img src="https://img.shields.io/crates/v/daemonbit.svg" alt="crates.io"></a>
  <a href="https://docs.rs/daemonbit"><img src="https://docs.rs/daemonbit/badge.svg" alt="docs.rs"></a>
  <a href="https://github.com/becmer/daemonbit/actions"><img src="https://github.com/becmer/daemonbit/actions/workflows/build.yml/badge.svg" alt="Build status"></a>
  <a href="https://github.com/becmer/daemonbit/blob/main/LICENSE-MIT"><img src="https://img.shields.io/badge/license-Apache--2.0%2FMIT-blue.svg" alt="license: MIT OR Apache-2.0"></a>
  <img src="https://img.shields.io/badge/status-WIP-orange" alt="Project status: WIP">
</p>

> **⚠️ Warning**: This project is in early development.

<br>

This crate provides the foundational primitives for managing cross-platform daemon lifecycles, with strict support for:

- **Lock enforcement**
    - `lockfile`-based locking on Unix (using `flock`)
    - `mutex`-based locking on Windows (using Win32 named mutexes)

- **Runtime directories**
    - Per-scope (`global`/`local`) runtime directory discovery and fallback
    - Cleanup on drop for temporary directories

The following major components are planned:

- **Cross-platform IPC**
    - Unix Domain Socket (UDS)
    - Named Pipe on Windows

- **System integration**
    - `systemd`
    - Windows SCM

These will build on the current abstractions, enabling full lifecycle management of daemons across platforms.

<br>

## MSRV Policy

This crate's Minimum Supported Rust Version (MSRV) is **1.85**, as required
by the 2024 edition. Changes to the MSRV will be accompanied by a minor
version bump.

The MSRV policy only takes effect beyond 1.85. Once applicable, it will be
bound by:

```
min(sid, stable - 3)
```

Where `sid` is the current version of `rustc` provided by [Debian Sid], and
`stable` is the latest [stable version] of Rust. This bound may be broken in
case of a major ecosystem shift or a security vulnerability.

[Debian Sid]: https://packages.debian.org/sid/rustc
[stable version]: https://releases.rs

<br>

#### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
