# Moonlight Web Server - Tester Guide

## Quick Start

### 1. Extract the Package

Extract `moonlight-web-server-win64.zip` to any folder. You'll get:
```
moonlight-web-server/
├── web-server.exe
└── dist/
    └── (frontend files)
```

### 2. Start the Server

Open PowerShell or Command Prompt in the extracted folder and run:

```powershell
.\web-server.exe
```

The server will start on **http://localhost:8080** by default.

### 3. Configure Sunshine/GFE

Make sure Sunshine (or GeForce Experience GameStream) is running on your PC with games available for streaming.

### 4. Connect

- **Browser:** Open http://localhost:8080 in Chrome/Edge
- **Android App:** Connect using the hybrid streaming mode with your server's IP address

---

## Server Options

```powershell
# Run on a different port
.\web-server.exe --port 9000

# Show help
.\web-server.exe --help
```

---

## Troubleshooting

### Server won't start
- Make sure no other application is using port 8080
- Try running as Administrator
- Check Windows Firewall isn't blocking the connection

### Can't connect from another device
- Use your PC's local IP address (e.g., `http://192.168.1.100:8080`)
- Make sure Windows Firewall allows incoming connections on port 8080
- Both devices must be on the same network

### Stream quality issues
- Check the stats overlay (toggle with the stats button or `MoonlightBridge.toggleStats()`)
- Look at Total Latency - should be under 60ms for good experience
- High "Buffer" latency indicates network instability

---

## Verifying Version

To confirm you're running the latest version, check the API version:

**In browser console:**
```javascript
MoonlightBridge.version  // Should return "1.3.0"
```

**For Android WebView:**
```kotlin
webView.evaluateJavascript("MoonlightBridge.version") { result -> 
    Log.d("Version", result)  // Should be "1.3.0"
}
```

---

## Reporting Issues

When reporting issues, please include:
1. `MoonlightBridge.version` output
2. Screenshot of stats overlay (if visible)
3. `MoonlightBridge.getStreamHealth()` output (for Android testers)
4. Device/browser information
5. Network setup (WiFi/Ethernet, same network or remote)

---

## Current Version: 1.3.0

**What's New:**
- Complete latency breakdown (Network, Encode, Streamer, Buffer, Decode)
- Accurate total latency calculation
- Color-coded quality indicators
- Stream health API for native apps
