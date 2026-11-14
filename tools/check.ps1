# Compile-checks and unit-tests the mod's api crate against the Horizon checkout in a scratch
# workspace, so no `git` dependency is fetched. The real crate's Cargo.toml (a git dependency) is
# what the launcher builds; this only swaps that one line for a path.
#
# Usage: powershell -File tools\check.ps1 [-Mode check|test|clippy] [-Horizon <Horizon checkout>]
#
# The Horizon checkout (at the commit horizon-mixin.toml pins) defaults to a `horizon` folder beside
# this repository; the scratch workspace and the build go beside it too.
param([string]$Mode = "test", [string]$Horizon = "")

$ErrorActionPreference = "Stop"
$dev = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$horizon = if ($Horizon) { $Horizon } else { Join-Path $dev "horizon" }
$ws = Join-Path $dev "check-ws"
$target = Join-Path $dev "target"
# The mod's source: api\ beside tools\, or realistic_tnt\api\ in the published repository.
$src = Join-Path $PSScriptRoot "..\api\src\lib.rs"
if (-not (Test-Path $src)) { $src = Join-Path $PSScriptRoot "..\realistic_tnt\api\src\lib.rs" }

New-Item -ItemType Directory -Force "$ws\api\src" | Out-Null
$horizonPath = (Join-Path $horizon "crates\horizon-client") -replace '\\', '/'
# Horizon builds with its own copies of wgpu-hal, naga and libmimalloc-sys (third_party/); a plain
# resolve picks the crates.io ones, and horizon-client-render does not compile against them.
$tp = (Join-Path $horizon "third_party") -replace '\\', '/'
Set-Content -Path "$ws\Cargo.toml" -Encoding utf8 -Value @"
[workspace]
resolver = "2"
members = ["api"]

[patch.crates-io]
wgpu-hal = { path = "$tp/wgpu-hal" }
naga = { path = "$tp/naga" }
libmimalloc-sys = { path = "$tp/libmimalloc-sys" }

[profile.dev]
debug = "line-tables-only"

[profile.dev.package.horizon-mod-api-gen]
opt-level = 3
[profile.dev.package.syn]
opt-level = 3
[profile.dev.package.proc-macro2]
opt-level = 3
"@
Set-Content -Path "$ws\api\Cargo.toml" -Encoding utf8 -Value @"
[package]
name = "mod-realistic_tnt"
version = "0.1.0"
edition = "2024"

[dependencies]
horizon-client = { path = "$horizonPath", default-features = false }
"@
Copy-Item $src "$ws\api\src\lib.rs" -Force
# Same dependency versions as the game: start from Horizon's own lockfile.
if (-not (Test-Path "$ws\Cargo.lock")) { Copy-Item (Join-Path $horizon "Cargo.lock") "$ws\Cargo.lock" }

Push-Location $ws
try {
    switch ($Mode) {
        "check"  { cargo check  --target-dir $target -p mod-realistic_tnt }
        "clippy" { cargo clippy --target-dir $target -p mod-realistic_tnt --all-targets -- -D warnings }
        default  { cargo test   --target-dir $target -p mod-realistic_tnt }
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
