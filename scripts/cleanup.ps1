#!/usr/bin/env pwsh
# Cleanup generated artifacts and test outputs.
# Safe to run at any time — only removes build/test byproducts.

param(
    [switch]$CargoClean
)

$ErrorActionPreference = "Continue"

Write-Host "Cleaning frontend coverage outputs..."
if (Test-Path "tauri-app/src-ui/coverage") {
    Remove-Item -Recurse -Force "tauri-app/src-ui/coverage"
}

Write-Host "Cleaning frontend build artifacts..."
if (Test-Path "tauri-app/src-ui/dist") {
    Remove-Item -Recurse -Force "tauri-app/src-ui/dist"
}
$viteCache = "tauri-app/src-ui/node_modules/.vite"
if (Test-Path $viteCache) {
    Remove-Item -Recurse -Force $viteCache
}

Write-Host "Cleaning lcov/coverage data..."
Get-ChildItem -Recurse -Filter "*.lcov" -ErrorAction SilentlyContinue | Remove-Item -Force
Get-ChildItem -Filter "lcov.info" -ErrorAction SilentlyContinue | Remove-Item -Force

if ($CargoClean) {
    Write-Host "Cleaning Rust build artifacts (cargo clean)..."
    cargo clean
}

Write-Host "Cleanup complete."
