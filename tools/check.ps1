# Checks the mod against the Horizon build it pins (horizon-mod.toml, [native] horizon), with that
# build's mod SDK: no Horizon checkout is needed. Any `cargo horizon-mod` command fetches the SDK into
# realistic_tnt\.horizon\sdk\ first.
#
# Usage: powershell -File tools\check.ps1 [-Mode check|test|clippy]
#
# Needs the mod tool:
#   cargo install --locked --git https://github.com/horizon-rust/horizon cargo-horizon-mod
# and, for its WebAssembly check, `rustup target add wasm32-unknown-unknown`.
param([string]$Mode = "test")

$ErrorActionPreference = "Stop"
Push-Location (Join-Path $PSScriptRoot "..\realistic_tnt")
try {
    cargo horizon-mod check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    switch ($Mode) {
        "check" { }
        "test" { cargo test }
        "clippy" { cargo clippy --all-targets -- -D warnings }
        default { throw "unknown mode: $Mode" }
    }
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
