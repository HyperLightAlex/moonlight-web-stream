# Backlight Desktop Client - Web Server Integration Specification

**Version:** 1.0  
**Date:** January 2026  
**Status:** Implementation Ready

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Architecture Overview](#2-architecture-overview)
3. [Component Specifications](#3-component-specifications)
4. [Process Lifecycle](#4-process-lifecycle)
5. [Authentication & Authorization](#5-authentication--authorization)
6. [Dual Platform Support](#6-dual-platform-support)
7. [Remote Streaming](#7-remote-streaming)
8. [Implementation Phases](#8-implementation-phases)

---

## 1. Executive Summary

### 1.1 Objective

Embed the moonlight-web-stream server and streamer components into the Backlight desktop client to provide:

- **Seamless WebRTC streaming** for Android clients via the Backbone mobile app
- **Continued Moonlight protocol support** for iOS clients via the custom Moonlight fork
- **Zero-configuration setup** for end users
- **Remote streaming capability** via WebRTC with STUN/UPnP (without requiring TURN infrastructure)

### 1.2 Key Outcomes

| Outcome | Description |
|---------|-------------|
| Single Installation | Users install only Backlight; web server is invisible |
| Auto-Pairing | Web server automatically pairs with embedded Sunshine |
| Dual Platform | Android (WebRTC) and iOS (Moonlight) both supported |
| Remote Access | Internet streaming via UPnP/STUN where possible |
| Credential Obfuscation | Users never interact with web server directly |

### 1.3 Out of Scope (Current Version)

- TURN server relay (infrastructure preserved for future)
- Multi-host management (single embedded Sunshine only)
- Web browser streaming UI (mobile clients only)

---

## 2. Architecture Overview

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        BACKLIGHT DESKTOP CLIENT                                  │
│                           (Single Executable)                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────────┐       │
│  │  Electron Main   │    │  Web Server      │    │  Bundled Sunshine    │       │
│  │  Process         │    │  (web-server.exe)│    │  (sunshine.exe)      │       │
│  │                  │    │                  │    │                      │       │
│  │  • Process mgmt  │───►│  • User/session  │◄──►│  • OTP endpoint      │       │
│  │  • QR generation │    │  • Host pairing  │    │  • Video encoding    │       │
│  │  • Settings UI   │    │  • WebRTC signal │    │  • Moonlight proto   │       │
│  └──────────────────┘    └────────┬─────────┘    └──────────────────────┘       │
│                                   │                                              │
│                    ┌──────────────▼──────────────┐                              │
│                    │  Streamer (streamer.exe)    │  Spawned per stream          │
│                    │  • WebRTC media transport   │  by web-server               │
│                    │  • Protocol translation     │                              │
│                    └─────────────────────────────┘                              │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
                    │                                        │
                    │ WebRTC                                 │ Moonlight Protocol
                    ▼                                        ▼
         ┌─────────────────────┐                 ┌─────────────────────┐
         │  Android Client     │                 │  iOS Client         │
         │  (Backbone App)     │                 │  (Moonlight Fork)   │
         └─────────────────────┘                 └─────────────────────┘
```

### 2.2 Network Ports

| Component | Port | Protocol | Purpose |
|-----------|------|----------|---------|
| Web Server | 8080 (configurable) | HTTP/WS | API, WebSocket signaling |
| Sunshine HTTP | 48989 | HTTP | Sunshine API, pairing |
| Sunshine HTTPS | 48990 | HTTPS | Secure API, streaming setup |
| Sunshine Stream | 47998-48000 | UDP | Moonlight game stream |
| Backlight WebSocket | 47995 | WS | iOS device communication |
| WebRTC Media | Dynamic (40000-40100) | UDP | WebRTC audio/video |

### 2.3 File System Layout

```
Backlight/
├── Backlight.exe                    # Main Electron executable
├── resources/
│   ├── sunshine/                    # Bundled Sunshine (existing)
│   │   ├── sunshine.exe
│   │   ├── assets/
│   │   └── config/
│   └── moonlight-web/               # NEW: Bundled web streamer
│       ├── web-server.exe
│       ├── streamer.exe
│       └── static/                  # Web UI assets (optional)
└── userData/                        # %APPDATA%/Backlight
    ├── sunshine-bundle/             # Sunshine runtime data (existing)
    ├── moonlight-web/               # NEW: Web server runtime data
    │   ├── config.json              # Server configuration
    │   └── data.json                # User/host/session storage
    └── settings.json                # Backlight settings (existing)
```

---

## 3. Component Specifications

### 3.1 Web Server (web-server.exe)

**Source:** `moonlight-web-stream/moonlight-web/web-server/`

#### 3.1.1 Responsibilities

- HTTP API for mobile client communication
- WebSocket server for real-time signaling
- User authentication and session management
- Host (Sunshine) pairing and management
- Spawning streamer subprocesses for active streams

#### 3.1.2 Configuration

The web server reads configuration from `config.json` in its working directory.

**Required Configuration for Backlight Integration:**

```json
{
  "data_storage": {
    "type": "json",
    "path": "./data.json",
    "session_expiration_check_interval": { "secs": 300, "nanos": 0 }
  },
  "web_server": {
    "bind_address": "0.0.0.0:8080",
    "first_login_create_admin": false,
    "first_login_assign_global_hosts": false,
    "default_user_id": null,
    "session_cookie_expiration": { "secs": 86400, "nanos": 0 },
    "session_cookie_secure": false
  },
  "moonlight": {
    "default_http_port": 48989,
    "pair_device_name": "Backlight-WebServer"
  },
  "webrtc": {
    "ice_servers": [
      {
        "urls": [
          "stun:stun.l.google.com:19302",
          "stun:stun1.l.google.com:3478",
          "stun:stun.cloudflare.com:3478"
        ]
      }
    ],
    "port_range": { "min": 40000, "max": 40100 },
    "network_types": ["udp4", "udp6"]
  },
  "upnp": {
    "enabled": true,
    "lease_duration_secs": 3600,
    "description": "Backlight Web Streaming"
  },
  "remote": {
    "enabled": true,
    "stun_discovery": true,
    "ssl_required": false
  },
  "turn": {
    "enabled": false,
    "urls": [],
    "username": "",
    "credential": ""
  },
  "streamer_path": "./streamer.exe",
  "log": {
    "level_filter": "Info",
    "file_path": "./logs/web-server.log"
  }
}
```

#### 3.1.3 Command Line Arguments

```bash
web-server.exe [OPTIONS]

OPTIONS:
    --config <PATH>          Path to config.json (default: ./server/config.json)
    --bind <ADDRESS:PORT>    Override bind address (e.g., 0.0.0.0:8080)
    --streamer <PATH>        Override streamer executable path
    --log-level <LEVEL>      Log level: Trace, Debug, Info, Warn, Error
```

**Backlight should invoke:**
```bash
web-server.exe --config "%APPDATA%/Backlight/moonlight-web/config.json" --bind "0.0.0.0:{AVAILABLE_PORT}"
```

### 3.2 Streamer (streamer.exe)

**Source:** `moonlight-web-stream/moonlight-web/streamer/`

#### 3.2.1 Responsibilities

- WebRTC peer connection management
- Receiving video/audio from Sunshine via Moonlight protocol
- Transcoding/forwarding media over WebRTC data channels
- Input handling (keyboard, mouse, gamepad)

#### 3.2.2 Invocation

The streamer is **not invoked directly by Backlight**. It is spawned by web-server.exe when a streaming session begins. The streamer communicates with the web server via stdin/stdout JSON IPC.

**No Backlight code changes needed for streamer management.**

### 3.3 Modifications Required to moonlight-web-stream

#### 3.3.1 New: Auto-Bootstrap Mode

Add a new startup mode for embedded deployment:

```rust
// New CLI flag
--bootstrap               Enable auto-bootstrap mode for embedded deployment
--bootstrap-user <NAME>   Username for auto-created user (default: "backlight")
--bootstrap-password <PW> Password for auto-created user (auto-generated if omitted)
--bootstrap-host <ADDR>   Host address to auto-add and pair (default: "localhost")
--bootstrap-host-port <P> Host HTTP port (default: 48989)
```

**Bootstrap Behavior:**
1. On first startup with `--bootstrap`:
   - Create user if not exists
   - Add localhost host if not exists
   - Attempt auto-pair with host (via OTP if Backlight Sunshine detected)
2. Output bootstrap result to stdout as JSON (for Backlight to capture):

```json
{
  "success": true,
  "user": {
    "id": 1,
    "username": "backlight",
    "password": "a3f8c2d1-b4e5-6789-abcd-ef0123456789"
  },
  "host": {
    "id": 1,
    "name": "Gaming PC",
    "paired": true
  },
  "server": {
    "port": 8080,
    "external_ip": "73.45.123.89",
    "upnp_success": true
  }
}
```

#### 3.3.2 New: Health Check Endpoint

Add a simple health check for Backlight to monitor:

```
GET /api/health

Response:
{
  "status": "ok",
  "uptime_secs": 3600,
  "active_streams": 0,
  "host_paired": true
}
```

#### 3.3.3 Existing: Fuji/Backlight Detection

Already implemented in `src/app/fuji.rs`. The web server detects Backlight hosts via the OTP endpoint and uses auto-pairing.

---

## 4. Process Lifecycle

### 4.1 Startup Sequence

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           BACKLIGHT STARTUP SEQUENCE                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  1. User launches Backlight.exe                                                  │
│     │                                                                            │
│     ▼                                                                            │
│  2. Electron main process initializes                                            │
│     │                                                                            │
│     ├──► 3a. Start bundled Sunshine (existing flow)                              │
│     │        └── Wait for Sunshine API ready (port 48989)                        │
│     │                                                                            │
│     └──► 3b. Find available port for web server                                  │
│              └── Check 8080, 8081, 8082... until available                       │
│                                                                                  │
│  4. Start web-server.exe                                                         │
│     │   └── Pass: --config, --bind, --bootstrap flags                            │
│     │                                                                            │
│     ▼                                                                            │
│  5. Web server bootstrap sequence                                                │
│     │   ├── Create default user (if not exists)                                  │
│     │   ├── Add localhost host (if not exists)                                   │
│     │   ├── Auto-pair with Sunshine via OTP                                      │
│     │   ├── Discover external IP via STUN                                        │
│     │   ├── Attempt UPnP port forwarding                                         │
│     │   └── Output bootstrap result JSON to stdout                               │
│     │                                                                            │
│     ▼                                                                            │
│  6. Backlight captures bootstrap result                                          │
│     │   ├── Store credentials in settings                                        │
│     │   ├── Store server port and external IP                                    │
│     │   └── Update UI with connection status                                     │
│     │                                                                            │
│     ▼                                                                            │
│  7. Backlight ready for mobile connections                                       │
│     └── QR codes available for Android and iOS                                   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Shutdown Sequence

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                          BACKLIGHT SHUTDOWN SEQUENCE                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  1. User closes Backlight (or system shutdown)                                   │
│     │                                                                            │
│     ▼                                                                            │
│  2. Electron 'before-quit' event fires                                           │
│     │                                                                            │
│     ├──► 3a. Send SIGTERM to web-server.exe                                      │
│     │        └── Web server gracefully closes active streams                     │
│     │        └── Streamer subprocesses terminated                                │
│     │                                                                            │
│     ├──► 3b. Send SIGTERM to Sunshine (existing flow)                            │
│     │                                                                            │
│     └──► 3c. Wait up to 5 seconds for processes                                  │
│              └── Force kill if not terminated                                    │
│                                                                                  │
│  4. Clean up UPnP port mappings (optional)                                       │
│     │                                                                            │
│     ▼                                                                            │
│  5. Electron process exits                                                       │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Crash Recovery

```typescript
// Backlight main process - web server management

class WebServerManager {
  private process: ChildProcess | null = null;
  private restartAttempts = 0;
  private maxRestartAttempts = 5;
  private restartDelayMs = 2000;

  async start(config: WebServerConfig): Promise<void> {
    const args = [
      '--config', config.configPath,
      '--bind', `0.0.0.0:${config.port}`,
      '--bootstrap',
      '--bootstrap-user', 'backlight',
    ];

    this.process = spawn(config.executablePath, args, {
      cwd: path.dirname(config.executablePath),
      stdio: ['ignore', 'pipe', 'pipe']
    });

    this.process.on('exit', (code, signal) => {
      log('web-server', `Process exited: code=${code}, signal=${signal}`);
      
      if (code !== 0 && this.restartAttempts < this.maxRestartAttempts) {
        this.restartAttempts++;
        log('web-server', `Restarting (attempt ${this.restartAttempts})...`);
        setTimeout(() => this.start(config), this.restartDelayMs);
      }
    });

    // Reset restart counter on successful operation
    this.process.on('spawn', () => {
      setTimeout(() => { this.restartAttempts = 0; }, 30000);
    });
  }

  async stop(): Promise<void> {
    if (this.process) {
      this.process.kill('SIGTERM');
      
      // Wait for graceful shutdown
      await new Promise(resolve => setTimeout(resolve, 3000));
      
      // Force kill if still running
      if (this.process && !this.process.killed) {
        this.process.kill('SIGKILL');
      }
    }
  }
}
```

---

## 5. Authentication & Authorization

### 5.1 Authentication Model

The web server uses session-based authentication. For the embedded Backlight scenario:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           AUTHENTICATION FLOW                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─────────────┐                      ┌─────────────┐                           │
│  │  Backlight  │                      │ Web Server  │                           │
│  │  Desktop    │                      │             │                           │
│  └──────┬──────┘                      └──────┬──────┘                           │
│         │                                    │                                   │
│         │  [First Launch - Bootstrap]        │                                   │
│         │                                    │                                   │
│         │  --bootstrap --bootstrap-user X    │                                   │
│         │───────────────────────────────────►│                                   │
│         │                                    │                                   │
│         │  { user: {id, username, password}, │                                   │
│         │    host: {id, name, paired} }      │                                   │
│         │◄───────────────────────────────────│                                   │
│         │                                    │                                   │
│         │  [Store credentials in settings]   │                                   │
│         │                                    │                                   │
│                                                                                  │
│  ┌─────────────┐                      ┌─────────────┐                           │
│  │  Android    │                      │ Web Server  │                           │
│  │  Client     │                      │             │                           │
│  └──────┬──────┘                      └──────┬──────┘                           │
│         │                                    │                                   │
│         │  [Scan QR Code - Get credentials]  │                                   │
│         │                                    │                                   │
│         │  POST /api/login                   │                                   │
│         │  { name: "backlight", password: X }│                                   │
│         │───────────────────────────────────►│                                   │
│         │                                    │                                   │
│         │  { session_token: "eyJ...",        │                                   │
│         │    remote_access: {...} }          │                                   │
│         │◄───────────────────────────────────│                                   │
│         │                                    │                                   │
│         │  [Store token, use for all calls]  │                                   │
│         │                                    │                                   │
│         │  GET /api/hosts                    │                                   │
│         │  Authorization: Bearer <token>     │                                   │
│         │───────────────────────────────────►│                                   │
│         │                                    │                                   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Session Regeneration

Sessions are regenerated when:
- Web server restarts
- Session expires (default: 24 hours)
- User explicitly logs out

**Android Client Handling:**

```kotlin
// Pseudo-code for Android client
class WebServerClient {
    private var sessionToken: String? = null
    private val credentials: Credentials // From QR code scan
    
    suspend fun <T> authenticatedRequest(request: () -> Response<T>): T {
        val response = request()
        
        if (response.code == 401) {
            // Session expired or invalid, re-authenticate
            sessionToken = login(credentials.username, credentials.password)
            return request() // Retry with new token
        }
        
        return response.body
    }
    
    private suspend fun login(username: String, password: String): String {
        val response = api.login(LoginRequest(username, password))
        return response.sessionToken
    }
}
```

### 5.3 Credential Security

| Location | What's Stored | Encryption |
|----------|---------------|------------|
| Backlight settings.json | Username, password (UUID) | Electron safeStorage |
| Android app | Username, password | Android Keystore |
| Web server data.json | Password hash (Argon2) | N/A (hashed) |

---

## 6. Dual Platform Support

### 6.1 QR Code Schemas

#### 6.1.1 Android QR Code (Web Server Auth)

```json
{
  "type": "backlight-webserver",
  "version": 1,
  "server": {
    "localUrl": "http://192.168.1.100:8080",
    "remoteUrl": "http://73.45.123.89:8080",
    "remoteAvailable": true
  },
  "credentials": {
    "username": "backlight",
    "password": "a3f8c2d1-b4e5-6789-abcd-ef0123456789"
  },
  "host": {
    "id": 1,
    "name": "Gaming PC"
  }
}
```

#### 6.1.2 iOS QR Code (Sunshine OTP - Existing)

```json
{
  "type": "fuji-pairing",
  "sessionId": "session_1705432100_abc123",
  "deviceId": "desktop_1705432100_xyz789",
  "deviceName": "Gaming PC - Backlight",
  "ipAddress": "192.168.1.100",
  "port": 47995,
  "expiresAt": 1705432400000,
  "sunshineOTP": {
    "pin": "1234",
    "passphrase": "desktop_1705432100_xyz789",
    "expiresAt": 1705432400000,
    "urls": {
      "http": "http://192.168.1.100:48989",
      "https": "https://192.168.1.100:48990"
    }
  }
}
```

### 6.2 UI Implementation

```typescript
// Backlight renderer - QR code generation

interface QRCodeState {
  activeTab: 'android' | 'ios';
  androidQR: string | null;
  iosQR: string | null;
  loading: boolean;
  error: string | null;
}

async function generateAndroidQR(): Promise<string> {
  const settings = await window.api.getSettings();
  const webServerInfo = await window.api.getWebServerInfo();
  
  const qrData = {
    type: 'backlight-webserver',
    version: 1,
    server: {
      localUrl: `http://${webServerInfo.localIp}:${webServerInfo.port}`,
      remoteUrl: webServerInfo.externalIp 
        ? `http://${webServerInfo.externalIp}:${webServerInfo.port}`
        : null,
      remoteAvailable: webServerInfo.upnpSuccess || webServerInfo.externalIp != null
    },
    credentials: {
      username: settings.webServer.username,
      password: settings.webServer.password
    },
    host: {
      id: 1,
      name: webServerInfo.hostName
    }
  };
  
  return JSON.stringify(qrData);
}

async function generateIOSQR(): Promise<string> {
  // Existing implementation - calls createPairingSession()
  const session = await window.api.createPairingSession();
  return JSON.stringify(session);
}
```

### 6.3 Platform Detection in Mobile Apps

**Android App:**
```kotlin
fun parseQRCode(data: String): PairingData {
    val json = JSONObject(data)
    
    return when (json.getString("type")) {
        "backlight-webserver" -> WebServerPairing(json)
        "fuji-pairing" -> throw UnsupportedOperationException("Use iOS app for Moonlight pairing")
        else -> throw IllegalArgumentException("Unknown QR code type")
    }
}
```

**iOS App:**
```swift
func parseQRCode(data: String) -> PairingData {
    let json = try JSONDecoder().decode(QRCodeData.self, from: data)
    
    switch json.type {
    case "fuji-pairing":
        return SunshinePairing(json)
    case "backlight-webserver":
        throw UnsupportedError("Use Android app for WebRTC streaming")
    default:
        throw ParseError("Unknown QR code type")
    }
}
```

---

## 7. Remote Streaming

### 7.1 Connection Flow

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        REMOTE STREAMING CONNECTION FLOW                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Android Client                Web Server                 Sunshine               │
│  (Mobile Network)              (Home PC)                  (Home PC)              │
│        │                           │                          │                  │
│        │  1. Login (remote URL)    │                          │                  │
│        │──────────────────────────►│                          │                  │
│        │                           │                          │                  │
│        │  2. Session + ICE servers │                          │                  │
│        │◄──────────────────────────│                          │                  │
│        │                           │                          │                  │
│        │  3. Start stream request  │                          │                  │
│        │──────────────────────────►│                          │                  │
│        │                           │                          │                  │
│        │                           │  4. Launch app           │                  │
│        │                           │─────────────────────────►│                  │
│        │                           │                          │                  │
│        │                           │  5. Streamer spawned     │                  │
│        │                           │◄─────────────────────────│                  │
│        │                           │                          │                  │
│        │  6. WebRTC offer          │                          │                  │
│        │◄──────────────────────────│                          │                  │
│        │                           │                          │                  │
│        │  7. WebRTC answer         │                          │                  │
│        │──────────────────────────►│                          │                  │
│        │                           │                          │                  │
│        │  8. ICE candidates        │                          │                  │
│        │◄─────────────────────────►│                          │                  │
│        │                           │                          │                  │
│        │  9. Direct P2P (STUN)     │                          │                  │
│        │◄═══════════════════════════════════════════════════►│                  │
│        │        OR                 │                          │                  │
│        │  9. Relayed (TURN)*       │                          │                  │
│        │◄════════════════════════►TURN◄═════════════════════►│                  │
│        │                           │                          │                  │
│                                                                                  │
│  * TURN only if enabled and configured (future feature)                          │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 NAT Traversal Strategy

| NAT Type | Detection | Strategy | Success Rate |
|----------|-----------|----------|--------------|
| Full Cone | STUN | Direct P2P | 99% |
| Restricted Cone | STUN | Direct P2P | 95% |
| Port Restricted | STUN | Direct P2P + UPnP | 80% |
| Symmetric | STUN | UPnP required | 50% |
| Symmetric + CGNAT | STUN | TURN only* | 0% (no TURN) |

### 7.3 User Guidance Messages

```typescript
// Messages to display based on remote access status

const remoteAccessMessages = {
  available: {
    title: "Remote Access Ready",
    message: "You can connect from anywhere using the external address.",
    icon: "check-circle"
  },
  upnp_failed: {
    title: "Manual Setup Required",
    message: "Automatic port forwarding failed. Please forward port {port} on your router to enable remote access.",
    icon: "warning",
    action: {
      label: "How to Port Forward",
      url: "https://support.backbone.com/port-forwarding"
    }
  },
  symmetric_nat: {
    title: "Limited Remote Access",
    message: "Your network uses Symmetric NAT which may prevent remote connections. Try using a VPN like Tailscale.",
    icon: "info",
    action: {
      label: "Setup Tailscale",
      url: "https://support.backbone.com/tailscale-setup"
    }
  },
  cgnat: {
    title: "Remote Access Unavailable",
    message: "Your ISP uses Carrier-Grade NAT. Contact your ISP to request a public IP, or use a VPN solution.",
    icon: "error"
  }
};
```

### 7.4 UPnP Configuration

The web server will attempt UPnP port forwarding on startup:

```json
{
  "upnp": {
    "enabled": true,
    "lease_duration_secs": 3600,
    "description": "Backlight Web Streaming",
    "webrtc_ports": { "min": 40000, "max": 40100 },
    "forward_tcp": false
  }
}
```

**Ports forwarded:**
- Web server HTTP port (e.g., 8080)
- WebRTC UDP port range (40000-40100)

---

## 8. Implementation Phases

### Phase 1: Core Embedding (Week 1-2)

**Objective:** Get web server running as a subprocess of Backlight

| Task | Owner | Effort | Dependencies |
|------|-------|--------|--------------|
| Compile web-server.exe for Windows | Backend | 2h | Cross toolchain |
| Compile streamer.exe for Windows | Backend | 2h | Cross toolchain |
| Add executables to Backlight resources | Desktop | 1h | Compiled binaries |
| Create WebServerManager class | Desktop | 4h | None |
| Implement process spawn/kill | Desktop | 2h | WebServerManager |
| Implement crash recovery | Desktop | 2h | Process management |
| Add health check polling | Desktop | 2h | `/api/health` endpoint |
| Dynamic port selection | Desktop | 2h | Port utils |
| Windows Firewall rule | Desktop | 1h | Existing firewall code |

**Deliverable:** Web server starts with Backlight and survives restarts

### Phase 2: Auto-Configuration (Week 2-3)

**Objective:** Zero-configuration first launch experience

| Task | Owner | Effort | Dependencies |
|------|-------|--------|--------------|
| Implement `--bootstrap` CLI flag | Backend | 4h | web-server codebase |
| Auto-create user on first launch | Backend | 2h | Bootstrap mode |
| Auto-add localhost host | Backend | 2h | Bootstrap mode |
| Auto-pair with Sunshine | Backend | 2h | Existing OTP code |
| Bootstrap JSON output | Backend | 2h | Bootstrap mode |
| Capture bootstrap result in Backlight | Desktop | 2h | Bootstrap output |
| Store credentials in settings | Desktop | 2h | Electron safeStorage |
| Implement `/api/health` endpoint | Backend | 1h | web-server codebase |

**Deliverable:** First launch creates user, pairs with Sunshine automatically

### Phase 3: Dual QR Code UI (Week 3-4)

**Objective:** Support both Android and iOS pairing flows

| Task | Owner | Effort | Dependencies |
|------|-------|--------|--------------|
| Design dual QR code UI | Design | 4h | None |
| Add Android QR generation | Desktop | 4h | Stored credentials |
| Add tab/toggle between QR types | Desktop | 2h | UI components |
| Update QR modal component | Desktop | 4h | Design spec |
| Display remote access status | Desktop | 2h | Web server info |
| Show user guidance messages | Desktop | 2h | NAT detection |

**Deliverable:** Users can pair Android (WebRTC) or iOS (Moonlight) devices

### Phase 4: Android Client Updates (Week 4-5)

**Objective:** Android app can authenticate and stream via web server

| Task | Owner | Effort | Dependencies |
|------|-------|--------|--------------|
| Parse new QR code schema | Android | 2h | Schema spec |
| Implement credential storage | Android | 2h | Android Keystore |
| Implement login flow | Android | 4h | API spec |
| Implement auto-reconnect | Android | 4h | Session handling |
| Test local streaming | Android | 4h | All above |
| Test remote streaming | Android | 4h | UPnP working |

**Deliverable:** Android app fully functional with embedded web server

### Phase 5: Testing & Polish (Week 5-6)

**Objective:** Production-ready integration

| Task | Owner | Effort | Dependencies |
|------|-------|--------|--------------|
| Integration testing | QA | 8h | All features |
| Performance testing | QA | 4h | Streaming working |
| NAT traversal testing | QA | 8h | Multiple networks |
| Error handling review | All | 4h | Integration tests |
| Documentation | All | 4h | Final implementation |
| Bug fixes | All | 8h | Testing results |

**Deliverable:** Production-ready embedded web server

---

## Next Document

Continue to [BACKLIGHT_INTEGRATION_API.md](./BACKLIGHT_INTEGRATION_API.md) for:
- Complete API reference
- WebSocket protocol specification
- Error codes and handling
- Configuration reference
