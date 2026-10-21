#!/usr/bin/env pwsh
# Cleanup generated artifacts and test outputs.
# Safe to run at any time — only removes build/test byproducts.

param(
    [switch]$CargoClean
)

$ErrorActionPreference = "Continue"

$frontendApps = @("hive-app", "entity-runtime-app", "legacy/tauri-app")

Write-Host "Cleaning frontend coverage outputs..."
foreach ($app in $frontendApps) {
    if (Test-Path "$app/src-ui/coverage") {
        Remove-Item -Recurse -Force "$app/src-ui/coverage"
    }
}

Write-Host "Cleaning frontend build artifacts..."
foreach ($app in $frontendApps) {
    foreach ($generated in @("$app/src-ui/dist", "$app/src-ui/node_modules/.vite")) {
        if (Test-Path $generated) {
            Remove-Item -Recurse -Force $generated
        }
    }
}

Write-Host "Cleaning lcov/coverage data..."
Get-ChildItem -Recurse -Filter "*.lcov" -ErrorAction SilentlyContinue | Remove-Item -Force
Get-ChildItem -Filter "lcov.info" -ErrorAction SilentlyContinue | Remove-Item -Force

if ($CargoClean) {
    Write-Host "Cleaning Rust build artifacts (cargo clean)..."
    cargo clean
}

Write-Host "Cleanup complete."
