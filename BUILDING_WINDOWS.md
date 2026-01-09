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

