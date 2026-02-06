# WebTransport API Verification Script
# This script helps verify the wtransport crate API calls

Write-Host "=== WebTransport API Verification ===" -ForegroundColor Cyan
Write-Host ""

# Step 1: Check if project compiles
Write-Host "Step 1: Checking compilation..." -ForegroundColor Yellow
Set-Location moonlight-web/streamer
$buildResult = cargo build 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "✓ Project compiles successfully" -ForegroundColor Green
} else {
    Write-Host "✗ Compilation errors found:" -ForegroundColor Red
    $buildResult | Select-String -Pattern "error" | Select-Object -First 10
    Write-Host ""
    Write-Host "Review the errors above to identify API issues." -ForegroundColor Yellow
    exit 1
}

Write-Host ""

# Step 2: Generate documentation
Write-Host "Step 2: Generating documentation..." -ForegroundColor Yellow
Write-Host "Run this command to view wtransport API docs:" -ForegroundColor Cyan
Write-Host "  cargo doc --open --package wtransport" -ForegroundColor White
Write-Host ""

# Step 3: List files with TODO comments
Write-Host "Step 3: Finding files with API verification TODOs..." -ForegroundColor Yellow
$todoFiles = Get-ChildItem -Recurse -Include *.rs | Select-String -Pattern "TODO.*wtransport|TODO.*API" | Select-Object -Unique Path
if ($todoFiles) {
    Write-Host "Files with API verification TODOs:" -ForegroundColor Cyan
    $todoFiles | ForEach-Object { Write-Host "  - $($_.Path)" -ForegroundColor White }
} else {
    Write-Host "✓ No TODO comments found" -ForegroundColor Green
}

Write-Host ""

# Step 4: Check wtransport version
Write-Host "Step 4: Checking wtransport version..." -ForegroundColor Yellow
$cargoLockPath = "Cargo.lock"
if (-not (Test-Path $cargoLockPath)) {
    $cargoLockPath = "moonlight-web\streamer\Cargo.lock"
}
if (Test-Path $cargoLockPath) {
    $cargoLock = Get-Content $cargoLockPath | Select-String -Pattern "wtransport" | Select-Object -First 3
if ($cargoLock) {
    Write-Host "wtransport version info:" -ForegroundColor Cyan
    $cargoLock | ForEach-Object { Write-Host "  $_" -ForegroundColor White }
} else {
    Write-Host "  Could not find Cargo.lock" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Next Steps ===" -ForegroundColor Cyan
Write-Host "1. Review WEBTRANSPORT_API_VERIFICATION.md for detailed verification steps" -ForegroundColor White
Write-Host "2. Open wtransport docs: cargo doc --open --package wtransport" -ForegroundColor White
Write-Host "3. Check specific API calls in the files listed above" -ForegroundColor White
Write-Host "4. Test with a browser connection to verify runtime behavior" -ForegroundColor White

Set-Location ../..
