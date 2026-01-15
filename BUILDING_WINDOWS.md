# Building Moonlight Web on Windows

This guide documents how to build and run Moonlight Web locally on Windows for development and testing.

## Prerequisites

The following tools must be installed:

| Tool | Purpose | Expected Location |
|------|---------|-------------------|
| **Rust (nightly)** | Compiles the backend | `%USERPROFILE%\.cargo\bin\` |
| **Node.js / npm** | Builds the frontend | In PATH |
| **Visual Studio Build Tools 2022** | C/C++ compiler (MSVC) | `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\` |
| **CMake** | Builds native dependencies | `C:\Program Files\CMake\bin\` |
| **Strawberry Perl** | Required for OpenSSL compilation | `C:\Strawberry\perl\bin\` |

## Build Steps

### 1. Open PowerShell and Set Up Environment

Run these commands to configure the build environment:

```powershell
# Initialize Visual Studio Developer environment (required for MSVC compiler)
& "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Launch-VsDevShell.ps1" -Arch amd64

# Add required tools to PATH
$env:Path = "C:\Strawberry\perl\bin;$env:USERPROFILE\.cargo\bin;C:\Program Files\CMake\bin;$env:Path"

# Use NMake generator (fixes MSVC detection issues with default VS generator)
$env:CMAKE_GENERATOR = "NMake Makefiles"
```

### 2. Build the Frontend

```powershell
cd moonlight-web\web-server
npm install
npm run build
```

This compiles TypeScript to JavaScript and copies static assets to `moonlight-web/web-server/dist/`.

> ⚠️ **IMPORTANT:** The `tsconfig.json` is in `moonlight-web/web-server/`, so TypeScript compilation must be run from that directory, NOT from `moonlight-web/web-server/web/`.

### 3. Build the Rust Backend

```powershell
cd C:\Users\ajkel\moonlight-web-stream  # or your project root
cargo build
```

For a release build (optimized):
```powershell
cargo build --release
```

### 4. Copy Frontend to Output Directory

The web server expects the frontend files in a `dist/` folder next to the executable:

```powershell
# For debug build
Copy-Item -Path "moonlight-web\web-server\dist" -Destination "target\debug\dist" -Recurse -Force

# For release build
Copy-Item -Path "moonlight-web\web-server\dist" -Destination "target\release\dist" -Recurse -Force
```

### 5. Run the Server

```powershell
# Debug build
cd target\debug
.\web-server.exe

# Release build
cd target\release
.\web-server.exe
```

The server will start on **http://localhost:8080** by default.

## Output File Locations

### Debug Build (`cargo build`)

| File | Location |
|------|----------|
| Web Server | `target\debug\web-server.exe` |
| Streamer | `target\debug\streamer.exe` |
| Frontend Assets | `target\debug\dist\` |
| Server Config | `target\debug\server\config.json` (created on first run) |

### Release Build (`cargo build --release`)

| File | Location |
|------|----------|
| Web Server | `target\release\web-server.exe` |
| Streamer | `target\release\streamer.exe` |
| Frontend Assets | `target\release\dist\` |
| Server Config | `target\release\server\config.json` (created on first run) |

## Quick Reference Script

Save this as `build-dev.ps1` in the project root for quick builds:

```powershell
# build-dev.ps1 - Quick development build script

# Setup environment
& "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Launch-VsDevShell.ps1" -Arch amd64
$env:Path = "C:\Strawberry\perl\bin;$env:USERPROFILE\.cargo\bin;C:\Program Files\CMake\bin;$env:Path"
$env:CMAKE_GENERATOR = "NMake Makefiles"

# Build frontend
Push-Location moonlight-web\web-server
npm install
npm run build
Pop-Location

# Build backend
cargo build

# Copy frontend to output
Copy-Item -Path "moonlight-web\web-server\dist" -Destination "target\debug\dist" -Recurse -Force

Write-Host "`nBuild complete! Run with: .\target\debug\web-server.exe" -ForegroundColor Green
```

## Frontend-Only Development

When making changes only to the web frontend (TypeScript/CSS), you don't need to rebuild the Rust backend:

### Quick Frontend Update

```powershell
# 1. Compile TypeScript (MUST run from web-server directory, not web/ subdirectory!)
cd moonlight-web\web-server
npx tsc

# 2. Copy static assets (CSS, HTML, etc.)
npm run copy-static

# 3. Deploy to running server
Copy-Item -Recurse -Force "dist\*" "..\..\target\debug\dist\"

# 4. Restart the server (or refresh browser if server auto-reloads)
Get-Process -Name "web-server" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
cd ..\..\target\debug
.\web-server.exe
```

### Verifying Frontend Updates

The frontend exposes a version number to help verify correct deployment:

```javascript
// In browser console or via Android WebView JS bridge:
MoonlightBridge.version  // Returns e.g., "1.2.0"
```

If you see old behavior after deploying, the issue is likely:
1. **Wrong build directory** - TypeScript must be compiled from `moonlight-web/web-server/`, not `moonlight-web/web-server/web/`
2. **Cached files** - Clear browser cache or WebView app data
3. **Didn't copy to target** - Frontend must be copied to `target/debug/dist/`

---

## Creating Distribution Packages

To package the web server for distribution to testers:

```powershell
cd C:\path\to\moonlight-web-stream

# Clean up old package
Remove-Item -Force "moonlight-web-server-win64.zip" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "package" -ErrorAction SilentlyContinue

# Create package directory
New-Item -ItemType Directory -Force -Path "package\moonlight-web-server" | Out-Null

# Copy executables and frontend
Copy-Item "target\debug\web-server.exe" "package\moonlight-web-server\"
Copy-Item "target\debug\streamer.exe" "package\moonlight-web-server\"
Copy-Item -Recurse "target\debug\dist" "package\moonlight-web-server\"

# Create zip archive
Compress-Archive -Path "package\moonlight-web-server\*" -DestinationPath "moonlight-web-server-win64.zip" -Force

Write-Host "Package created: moonlight-web-server-win64.zip"
Get-ChildItem "package\moonlight-web-server\*.exe" | Select-Object Name, Length
```

The resulting `moonlight-web-server-win64.zip` contains:

| File | Purpose |
|------|---------|
| `web-server.exe` | Main server (web UI, pairing, API) |
| `streamer.exe` | Game streaming process (spawned by web-server) |
| `dist/` | Frontend assets (HTML, JS, CSS) |

> ⚠️ **IMPORTANT:** Both executables must be in the same folder! The web server spawns `streamer.exe` when launching a stream.

> **Note:** The `package/` folder and `*.zip` files are excluded from git (see `.gitignore`).

### Quick Package Script

Save this as `package.ps1` in the project root:

```powershell
# package.ps1 - Create distribution package for testers

$ErrorActionPreference = "Stop"

# Clean up
Remove-Item -Force "moonlight-web-server-win64.zip" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "package" -ErrorAction SilentlyContinue

# Create package
New-Item -ItemType Directory -Force -Path "package\moonlight-web-server" | Out-Null
Copy-Item "target\debug\web-server.exe" "package\moonlight-web-server\"
Copy-Item "target\debug\streamer.exe" "package\moonlight-web-server\"
Copy-Item -Recurse "target\debug\dist" "package\moonlight-web-server\"

# Create zip
Compress-Archive -Path "package\moonlight-web-server\*" -DestinationPath "moonlight-web-server-win64.zip" -Force

# Report
$size = [math]::Round((Get-Item "moonlight-web-server-win64.zip").Length / 1MB, 1)
Write-Host "`nPackage created: moonlight-web-server-win64.zip ($size MB)" -ForegroundColor Green
Get-ChildItem "package\moonlight-web-server" | Format-Table Name, @{N='Size (MB)';E={[math]::Round($_.Length/1MB,1)}}
```

---

## Hybrid Mode / Android WebView Development

When developing for the hybrid streaming mode (Android WebView):

### Clearing WebView Cache

Android WebView aggressively caches JavaScript files. After updating the frontend:

1. **On Android device:** Settings → Apps → [Your App] → Storage → Clear Cache (or Clear Data)
2. **Or in-app:** Implement cache-clearing in your Android app

### Checking Deployed Version

```javascript
// Call from Android via evaluateJavascript:
MoonlightBridge.version
```

### MoonlightBridge API

The frontend exposes `window.MoonlightBridge` for native Android integration:

| Method | Description |
|--------|-------------|
| `version` | Returns API version string (e.g., "1.2.0") |
| `getStreamHealth()` | Returns JSON with quality metrics (async) |
| `toggleStats()` | Shows/hides detailed stats overlay |
| `isStatsVisible()` | Returns boolean |
| `getTouchMode()` / `setTouchMode()` | Touch input mode |
| `getMouseMode()` / `setMouseMode()` | Mouse input mode |
| `showKeyboard()` / `hideKeyboard()` | Soft keyboard control |
| `sendText(text)` | Send text input |
| `sendKey(isDown, keyCode, modifiers)` | Send key events |

---

## Troubleshooting

### "cargo not found"
Add Cargo to PATH: `$env:Path += ";$env:USERPROFILE\.cargo\bin"`

### "perl not found" (OpenSSL build fails)
Add Strawberry Perl to PATH: `$env:Path = "C:\Strawberry\perl\bin;$env:Path"`

### "cmake not found"
Add CMake to PATH: `$env:Path += ";C:\Program Files\CMake\bin"`

### "No CMAKE_C_COMPILER could be found"
Two options:
1. Run from VS Developer PowerShell (recommended)
2. Set the generator: `$env:CMAKE_GENERATOR = "NMake Makefiles"`

### "Specified path is not a directory: dist"
Copy the frontend build output to the executable directory:
```powershell
Copy-Item -Path "moonlight-web\web-server\dist" -Destination "target\debug\dist" -Recurse -Force
```

### Frontend changes not appearing / Old code running

**Symptoms:**
- Old console.log messages appearing
- Old behavior despite code changes
- Version number not updated

**Causes & Solutions:**

1. **Built from wrong directory:**
   ```powershell
   # WRONG - creates no output!
   cd moonlight-web\web-server\web
   npx tsc
   
   # CORRECT - tsconfig.json is here
   cd moonlight-web\web-server
   npx tsc
   ```

2. **Didn't copy to target directory:**
   ```powershell
   Copy-Item -Recurse -Force "moonlight-web\web-server\dist\*" "target\debug\dist\"
   ```

3. **Browser/WebView cache:**
   - Browser: Hard refresh (Ctrl+Shift+R) or clear cache
   - Android WebView: Clear app data/cache on device

4. **Server not restarted:**
   ```powershell
   Get-Process -Name "web-server" -ErrorAction SilentlyContinue | Stop-Process -Force
   ```

### WebRTC stats showing wrong values (e.g., latency in seconds instead of ms)

WebRTC reports many values in **seconds** (as floats), not milliseconds:
- `currentRoundTripTime` - seconds (multiply by 1000 for ms)
- `jitter` - seconds (multiply by 1000 for ms)
- `totalDecodeTime` - cumulative seconds (divide by `framesDecoded`, then multiply by 1000)
- `jitterBufferDelay` - cumulative seconds (divide by `jitterBufferEmittedCount`, then multiply by 1000)

### Stats overlay not showing in hybrid mode

Check `styles.css` for rules hiding elements in hybrid mode:
```css
/* This will hide stats - remove .video-stats if you want it visible */
body.hybrid-mode .video-stats {
    display: none !important;
}
```
