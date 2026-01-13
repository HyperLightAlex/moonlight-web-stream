# Moonlight Web Server - Tester Guide

## Quick Start

### 1. Extract the Package

Extract `moonlight-web-server-win64.zip` to any folder. You'll get:
```
moonlight-web-server/
├── web-server.exe    (web UI & pairing)
├── streamer.exe      (handles game streaming)
└── dist/
    └── (frontend files)
```

**Important:** Both `.exe` files must be in the same folder!

### 2. Start the Server

Open PowerShell or Command Prompt in the extracted folder and run:

```powershell
.\web-server.exe
```

The server will start on **http://localhost:8080** by default.

### 3. Configure Sunshine/GFE

Make sure Sunshine (or GeForce Experience GameStream) is running on your PC with games available for streaming.

### 4. Connect

- **Browser:** 
- Open http://localhost:8080 in Chrome/Edge, if opening for the first time you will be prompted to create user credentials(suggest using something simple)
- after login, host list will appear. to add new host click the + host icon
- for Vanilla sunshine hosts enter: "localhost" with empty port
- for backlight hosts enter: "localhost" with "48989" for port
- all unpaired hosts will show in list with a locked icon 
- to pair host, select it from the host list. Vanilla sunshine hosts will require PIN entry via sunshine webUI, backlight hosts should auto pair after selection

- **Android App:** (local network only)
- make sure server is running on host PC and open server web UI in browser
- open backbone app to homescreen and navigate to PC Games row
- select "Discovery PCs"
- select host server from list with local IP address
- if not paired or session expired, QR code scanner should display
- select "QR code" from sevrer web UI on host browser, enter user password, qr code should appear
- scan QR code within backbone app
- on successful pair games list should appear for all paired hosts

- To launch custom WebRTC fork streaming flow: launch PC game from PC discovery flow games list
- To launch WebView streaming flow: launch PC game from either homescreen PC Games row, or PC game from library flow

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
