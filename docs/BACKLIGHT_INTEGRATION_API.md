# Backlight Integration - API Reference

**Version:** 1.0  
**Date:** January 2026  
**Parent Document:** [BACKLIGHT_INTEGRATION.md](./BACKLIGHT_INTEGRATION.md)

---

## Table of Contents

1. [API Overview](#1-api-overview)
2. [Authentication API](#2-authentication-api)
3. [Host Management API](#3-host-management-api)
4. [Streaming API](#4-streaming-api)
5. [Admin API](#5-admin-api)
6. [WebSocket Protocol](#6-websocket-protocol)
7. [Error Handling](#7-error-handling)
8. [Configuration Reference](#8-configuration-reference)

---

## 1. API Overview

### 1.1 Base URL

```
Local:  http://{LOCAL_IP}:{PORT}
Remote: http://{EXTERNAL_IP}:{PORT}
```

Default port: `8080` (configurable)

### 1.2 Authentication

All API endpoints (except `/api/login`) require authentication via:

- **Bearer Token**: `Authorization: Bearer <session_token>`
- **Session Cookie**: `mlSession=<session_token>` (browser clients)

### 1.3 Content Types

- Request: `application/json`
- Response: `application/json` or `application/x-ndjson` (streaming endpoints)

### 1.4 Common Response Format

**Success:**
```json
{
  "field": "value"
}
```

**Error:**
```json
{
  "error": "ErrorCode",
  "message": "Human readable description"
}
```

---

## 2. Authentication API

### 2.1 Login

Authenticate with username and password to obtain a session token.

```http
POST /api/login
Content-Type: application/json

{
  "name": "backlight",
  "password": "a3f8c2d1-b4e5-6789-abcd-ef0123456789"
}
```

**Response (200 OK):**
```json
{
  "session_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "remote_access": {
    "external_ip": "73.45.123.89",
    "hostname": null,
    "port": 8080,
    "ssl_available": false,
    "discovery_method": "stun",
    "nat_type": "restricted_cone",
    "turn_recommended": false,
    "ice_servers": [
      {
        "urls": ["stun:stun.l.google.com:19302"]
      }
    ]
  }
}
```

**Response (401 Unauthorized):**
```json
{
  "error": "InvalidCredentials",
  "message": "Invalid username or password"
}
```

### 2.2 Logout

Invalidate the current session.

```http
POST /api/logout
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```
(empty body)
```

### 2.3 Authenticate (Validate Session)

Check if the current session is valid and get remote access info.

```http
GET /api/authenticate
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```json
{
  "remote_access": {
    "external_ip": "73.45.123.89",
    "port": 8080,
    "nat_type": "restricted_cone",
    "turn_recommended": false
  }
}
```

**Response (401 Unauthorized):**
```json
{
  "error": "SessionTokenNotFound",
  "message": "Session expired or invalid"
}
```

---

## 3. Host Management API

### 3.1 List Hosts

Get all hosts for the authenticated user. Returns streaming NDJSON.

```http
GET /api/hosts
Authorization: Bearer <session_token>
```

**Response (200 OK, NDJSON):**

Line 1 - Initial cached state:
```json
{"hosts":[{"host_id":1,"owner":"ThisUser","name":"Gaming PC","paired":"Paired","server_state":null}]}
```

Line 2+ - Live updates per host:
```json
{"host_id":1,"owner":"ThisUser","name":"Gaming PC","paired":"Paired","server_state":"Free"}
```

### 3.2 Get Host Details

Get detailed information about a specific host.

```http
GET /api/host?host_id=1
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```json
{
  "host": {
    "host_id": 1,
    "owner": "ThisUser",
    "name": "Gaming PC",
    "paired": "Paired",
    "server_state": "Free",
    "host_type": "Backlight",
    "address": "localhost",
    "http_port": 48989,
    "https_port": 48990,
    "external_port": 47998,
    "version": "2024.1201.1",
    "gfe_version": "3.0.0",
    "unique_id": "ABC123DEF456",
    "mac": "AA:BB:CC:DD:EE:FF",
    "local_ip": "192.168.1.100",
    "current_game": 0,
    "max_luma_pixels_hevc": 0,
    "server_codec_mode_support": 3
  }
}
```

### 3.3 Add Host

Add a new host to the user's host list.

```http
POST /api/host
Authorization: Bearer <session_token>
Content-Type: application/json

{
  "address": "192.168.1.100",
  "http_port": 48989
}
```

**Response (200 OK):**
```json
{
  "host": {
    "host_id": 2,
    "name": "New Host",
    "paired": "NotPaired",
    "server_state": "Free",
    "address": "192.168.1.100",
    "http_port": 48989,
    "https_port": 48990
  }
}
```

### 3.4 Delete Host

Remove a host from the user's host list.

```http
DELETE /api/host?host_id=2
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```json
{
  "success": true
}
```

### 3.5 Pair with Host

Initiate pairing with a host. Returns streaming NDJSON with pairing status.

```http
POST /api/pair
Authorization: Bearer <session_token>
Content-Type: application/json

{
  "host_id": 1
}
```

**Response (200 OK, NDJSON):**

Line 1 - Pairing type:
```json
"FujiAutoPairing"
```
*OR for standard Sunshine:*
```json
{"Pin":"1234"}
```

Line 2 - Pairing result:
```json
{"Paired":{"host_id":1,"name":"Gaming PC","paired":"Paired"}}
```
*OR on failure:*
```json
"PairError"
```

### 3.6 Wake Host (Wake-on-LAN)

Send Wake-on-LAN magic packet to a host.

```http
POST /api/wake
Authorization: Bearer <session_token>
Content-Type: application/json

{
  "host_id": 1
}
```

**Response (200 OK):**
```json
{
  "success": true
}
```

---

## 4. Streaming API

### 4.1 List Applications

Get available applications/games from a paired host.

```http
GET /api/apps?host_id=1
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```json
{
  "apps": [
    {
      "app_id": 1,
      "title": "Desktop",
      "is_hdr_supported": false
    },
    {
      "app_id": 2,
      "title": "Cyberpunk 2077",
      "is_hdr_supported": true
    },
    {
      "app_id": 3,
      "title": "Elden Ring",
      "is_hdr_supported": false
    }
  ]
}
```

### 4.2 Get Application Box Art

Get the box art image for an application.

```http
GET /api/app_image?host_id=1&app_id=2
Authorization: Bearer <session_token>
```

**Response (200 OK):**
```
Content-Type: image/png
(binary image data)
```

### 4.3 Cancel Running Application

Stop the currently running application on a host.

```http
POST /api/host/cancel
Authorization: Bearer <session_token>
Content-Type: application/json

{
  "host_id": 1
}
```

**Response (200 OK):**
```json
{
  "success": true
}
```

### 4.4 Start Stream (WebSocket)

Initiate a streaming session via WebSocket upgrade.

```
GET /api/host/stream
Authorization: Bearer <session_token>
Upgrade: websocket
Connection: Upgrade
```

See [Section 6: WebSocket Protocol](#6-websocket-protocol) for message format.

---

## 5. Admin API

*Note: Admin APIs are typically not needed for Backlight integration but are documented for completeness.*

### 5.1 List Users (Admin Only)

```http
GET /api/admin/users
Authorization: Bearer <admin_session_token>
```

**Response (200 OK):**
```json
{
  "users": [
    {
      "id": 1,
      "username": "backlight",
      "is_admin": true
    }
  ]
}
```

### 5.2 Delete User (Admin Only)

```http
DELETE /api/admin/user?user_id=2
Authorization: Bearer <admin_session_token>
```

---

## 6. WebSocket Protocol

### 6.1 Connection

```javascript
const ws = new WebSocket('ws://{HOST}:{PORT}/api/host/stream');
// OR with auth:
const ws = new WebSocket('ws://{HOST}:{PORT}/api/host/stream', [], {
  headers: { 'Authorization': 'Bearer ' + sessionToken }
});
```

### 6.2 Message Flow

```
Client                              Server
  │                                    │
  │  1. Connect (WebSocket upgrade)    │
  │───────────────────────────────────►│
  │                                    │
  │  2. Init (stream parameters)       │
  │───────────────────────────────────►│
  │                                    │
  │  3. StageStarting                  │
  │◄───────────────────────────────────│
  │                                    │
  │  4. UpdateApp                      │
  │◄───────────────────────────────────│
  │                                    │
  │  5. IceServers                     │
  │◄───────────────────────────────────│
  │                                    │
  │  6. WebRTC Offer                   │
  │◄───────────────────────────────────│
  │                                    │
  │  7. WebRTC Answer                  │
  │───────────────────────────────────►│
  │                                    │
  │  8. ICE Candidates (bidirectional) │
  │◄──────────────────────────────────►│
  │                                    │
  │  9. ConnectionComplete             │
  │◄───────────────────────────────────│
  │                                    │
  │  10. (Stream data via WebRTC)      │
  │◄══════════════════════════════════►│
  │                                    │
```

### 6.3 Client Messages

#### Init

First message sent by client after connection.

```json
{
  "Init": {
    "host_id": 1,
    "app_id": 2,
    "bitrate": 20000,
    "packet_size": 1024,
    "fps": 60,
    "width": 1920,
    "height": 1080,
    "video_frame_queue_size": 5,
    "play_audio_local": false,
    "audio_sample_queue_size": 10,
    "video_supported_formats": 7,
    "video_colorspace": "Rec709",
    "video_color_range_full": false,
    "hybrid_mode": false
  }
}
```

**Field Reference:**

| Field | Type | Description |
|-------|------|-------------|
| host_id | u32 | Target host ID |
| app_id | u32 | Application to launch |
| bitrate | u32 | Target bitrate in kbps |
| packet_size | u32 | UDP packet size |
| fps | u32 | Target frame rate |
| width | u32 | Stream width in pixels |
| height | u32 | Stream height in pixels |
| video_frame_queue_size | u32 | Video buffer size |
| play_audio_local | bool | Play audio on host PC |
| audio_sample_queue_size | u32 | Audio buffer size |
| video_supported_formats | u32 | Bitflags: H264=1, HEVC=2, AV1=4 |
| video_colorspace | string | "Rec601", "Rec709", "Rec2020" |
| video_color_range_full | bool | Full vs limited range |
| hybrid_mode | bool | Separate input connection |

#### WebRTC Answer

```json
{
  "WebRtcAnswer": {
    "sdp": "v=0\r\no=- 123456 2 IN IP4 127.0.0.1\r\n..."
  }
}
```

#### ICE Candidate

```json
{
  "WebRtcIceCandidate": {
    "candidate": "candidate:1 1 UDP 2122252543 192.168.1.100 40001 typ host",
    "sdp_mid": "0",
    "sdp_mline_index": 0
  }
}
```

### 6.4 Server Messages

#### StageStarting

```json
{
  "StageStarting": {
    "stage": "Launch Streamer"
  }
}
```

#### UpdateApp

```json
{
  "UpdateApp": {
    "app": {
      "app_id": 2,
      "title": "Cyberpunk 2077",
      "is_hdr_supported": true
    }
  }
}
```

#### IceServers

```json
{
  "IceServers": {
    "ice_servers": [
      {
        "urls": ["stun:stun.l.google.com:19302"]
      }
    ]
  }
}
```

#### WebRTC Offer

```json
{
  "WebRtcOffer": {
    "sdp": "v=0\r\no=- 123456 2 IN IP4 127.0.0.1\r\n..."
  }
}
```

#### ICE Candidate

```json
{
  "WebRtcIceCandidate": {
    "candidate": "candidate:1 1 UDP 2122252543 192.168.1.100 40001 typ host",
    "sdp_mid": "0",
    "sdp_mline_index": 0
  }
}
```

#### ConnectionComplete

```json
{
  "ConnectionComplete": {
    "format": 2,
    "width": 1920,
    "height": 1080,
    "fps": 60,
    "capabilities": {
      "mouse_relative_supported": true,
      "keyboard_lock_supported": true,
      "gamepad_supported": true
    }
  }
}
```

#### Error Messages

```json
"HostNotFound"
"AppNotFound"
"HostNotPaired"
"InternalServerError"
```

---

## 7. Error Handling

### 7.1 HTTP Status Codes

| Code | Meaning | Common Causes |
|------|---------|---------------|
| 200 | Success | Request processed successfully |
| 400 | Bad Request | Invalid JSON, missing fields |
| 401 | Unauthorized | Invalid/expired session token |
| 403 | Forbidden | User doesn't have permission |
| 404 | Not Found | Host/app not found |
| 500 | Internal Error | Server-side error |
| 504 | Gateway Timeout | Host offline |

### 7.2 Error Codes

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `InvalidCredentials` | 401 | Username/password incorrect |
| `SessionTokenNotFound` | 401 | Session expired or invalid |
| `SessionTokenMalformed` | 401 | Token format invalid |
| `Forbidden` | 403 | Insufficient permissions |
| `HostNotFound` | 404 | Host ID doesn't exist |
| `HostNotPaired` | 400 | Host not paired yet |
| `HostPaired` | 400 | Host already paired |
| `HostOffline` | 504 | Host unreachable |
| `AppNotFound` | 404 | Application ID invalid |
| `FujiPairingFailed` | 500 | Auto-pairing failed |

### 7.3 Android Client Error Handling

```kotlin
sealed class ApiResult<T> {
    data class Success<T>(val data: T) : ApiResult<T>()
    data class Error<T>(val code: String, val message: String) : ApiResult<T>()
}

suspend fun <T> safeApiCall(call: suspend () -> Response<T>): ApiResult<T> {
    return try {
        val response = call()
        
        when {
            response.isSuccessful -> ApiResult.Success(response.body()!!)
            response.code() == 401 -> {
                // Attempt re-authentication
                val reauth = reAuthenticate()
                if (reauth) {
                    // Retry original call
                    safeApiCall(call)
                } else {
                    ApiResult.Error("SessionExpired", "Please scan QR code again")
                }
            }
            else -> {
                val error = parseError(response.errorBody())
                ApiResult.Error(error.code, error.message)
            }
        }
    } catch (e: IOException) {
        ApiResult.Error("NetworkError", "Unable to connect to server")
    }
}
```

---

## 8. Configuration Reference

### 8.1 Server Configuration (config.json)

```json
{
  "data_storage": {
    "type": "json",
    "path": "./data.json",
    "session_expiration_check_interval": {
      "secs": 300,
      "nanos": 0
    }
  },
  "web_server": {
    "bind_address": "0.0.0.0:8080",
    "certificate": null,
    "url_path_prefix": "",
    "session_cookie_secure": false,
    "session_cookie_expiration": {
      "secs": 86400,
      "nanos": 0
    },
    "first_login_create_admin": false,
    "first_login_assign_global_hosts": false,
    "default_user_id": null,
    "forwarded_header": null
  },
  "moonlight": {
    "default_http_port": 48989,
    "pair_device_name": "Backlight-WebServer"
  },
  "webrtc": {
    "ice_servers": [
      {
        "is_default": true,
        "urls": [
          "stun:stun.l.google.com:19302",
          "stun:stun1.l.google.com:3478",
          "stun:stun.cloudflare.com:3478"
        ],
        "username": "",
        "credential": ""
      }
    ],
    "port_range": {
      "min": 40000,
      "max": 40100
    },
    "nat_1to1": null,
    "network_types": ["udp4", "udp6"],
    "include_loopback_candidates": true
  },
  "upnp": {
    "enabled": true,
    "lease_duration_secs": 3600,
    "description": "Backlight Web Streaming",
    "webrtc_ports": null,
    "forward_tcp": false
  },
  "remote": {
    "enabled": true,
    "hostname": null,
    "port": null,
    "ssl_required": false,
    "stun_discovery": true
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

### 8.2 Configuration Field Reference

#### data_storage

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| type | string | "json" | Storage backend type |
| path | string | "./server/data.json" | Data file path |
| session_expiration_check_interval | Duration | 5 min | Session cleanup interval |

#### web_server

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| bind_address | string | "0.0.0.0:8080" | Listen address and port |
| certificate | object | null | SSL certificate config |
| url_path_prefix | string | "" | URL prefix for reverse proxy |
| session_cookie_secure | bool | false | Require HTTPS for cookies |
| session_cookie_expiration | Duration | 24h | Session lifetime |
| first_login_create_admin | bool | true | Auto-create admin user |
| default_user_id | u32 | null | Skip login for this user |

#### webrtc

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| ice_servers | array | Google STUN | ICE server configuration |
| port_range | object | null | Restrict UDP ports |
| nat_1to1 | object | null | Manual NAT mapping |
| network_types | array | ["udp4", "udp6"] | Allowed network types |

#### upnp

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| enabled | bool | false | Enable UPnP port forwarding |
| lease_duration_secs | u32 | 3600 | Port mapping lease time |
| description | string | "Moonlight Web Stream" | Router display name |

#### remote

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| enabled | bool | true | Include remote info in login |
| hostname | string | null | Custom hostname/DDNS |
| stun_discovery | bool | true | Use STUN for external IP |

### 8.3 Command Line Arguments

```
USAGE:
    web-server.exe [OPTIONS]

OPTIONS:
    -c, --config <PATH>
            Path to configuration file
            [default: ./server/config.json]

    -b, --bind <ADDRESS>
            Override bind address (e.g., 0.0.0.0:8080)

    -s, --streamer <PATH>
            Override streamer executable path

    -l, --log-level <LEVEL>
            Log level: Trace, Debug, Info, Warn, Error
            [default: Info]

    --bootstrap
            Enable bootstrap mode for embedded deployment

    --bootstrap-user <NAME>
            Username for auto-created user
            [default: backlight]

    --bootstrap-password <PASSWORD>
            Password for auto-created user
            [default: auto-generated UUID]

    --bootstrap-host <ADDRESS>
            Host address to auto-add and pair
            [default: localhost]

    --bootstrap-host-port <PORT>
            Host HTTP port
            [default: 48989]

    --disable-default-webrtc-ice-servers
            Don't use default STUN servers

    --webrtc-nat-1to1-host <IP>
            Advertise IP as host ICE candidate

    -h, --help
            Print help information

    -V, --version
            Print version information
```

### 8.4 Environment Variables

| Variable | Description |
|----------|-------------|
| `WEBRTC_ICE_SERVER_0_URL` | First ICE server URL |
| `WEBRTC_ICE_SERVER_0_USERNAME` | First ICE server username |
| `WEBRTC_ICE_SERVER_0_CREDENTIAL` | First ICE server credential |
| `WEBRTC_NAT_1TO1_HOST` | Host NAT 1:1 IP mapping |
| `DISABLE_DEFAULT_WEBRTC_ICE_SERVERS` | Skip default STUN servers |

---

## Appendix A: TypeScript Type Definitions

For Android/iOS client development, here are the corresponding TypeScript types:

```typescript
// Authentication
interface LoginRequest {
  name: string;
  password: string;
}

interface LoginResponse {
  session_token: string;
  remote_access: RemoteAccessInfo | null;
}

interface RemoteAccessInfo {
  external_ip: string | null;
  hostname: string | null;
  port: number;
  ssl_available: boolean;
  discovery_method: string;
  nat_type: string;
  turn_recommended: boolean;
  ice_servers: RtcIceServer[] | null;
}

interface RtcIceServer {
  urls: string[];
  username?: string;
  credential?: string;
}

// Hosts
type HostOwner = "ThisUser" | "Global";
type PairStatus = "Paired" | "NotPaired";
type HostState = "Free" | "Busy";
type HostType = "Backlight" | "Standard";

interface UndetailedHost {
  host_id: number;
  owner: HostOwner;
  name: string;
  paired: PairStatus;
  server_state: HostState | null;
}

interface DetailedHost extends UndetailedHost {
  host_type: HostType | null;
  address: string;
  http_port: number;
  https_port: number;
  external_port: number;
  version: string;
  gfe_version: string;
  unique_id: string;
  mac: string | null;
  local_ip: string;
  current_game: number;
  max_luma_pixels_hevc: number;
  server_codec_mode_support: number;
}

// Applications
interface App {
  app_id: number;
  title: string;
  is_hdr_supported: boolean;
}

// Pairing
interface PairRequest {
  host_id: number;
}

type PairResponse1 = 
  | "FujiAutoPairing"
  | { Pin: string }
  | "InternalServerError"
  | "PairError";

type PairResponse2 = 
  | { Paired: DetailedHost }
  | "PairError";

// Stream Init
interface StreamInit {
  host_id: number;
  app_id: number;
  bitrate: number;
  packet_size: number;
  fps: number;
  width: number;
  height: number;
  video_frame_queue_size: number;
  play_audio_local: boolean;
  audio_sample_queue_size: number;
  video_supported_formats: number;
  video_colorspace: "Rec601" | "Rec709" | "Rec2020";
  video_color_range_full: boolean;
  hybrid_mode: boolean;
}

// QR Code Schemas
interface AndroidQRCode {
  type: "backlight-webserver";
  version: number;
  server: {
    localUrl: string;
    remoteUrl: string | null;
    remoteAvailable: boolean;
  };
  credentials: {
    username: string;
    password: string;
  };
  host: {
    id: number;
    name: string;
  };
}

interface IOSQRCode {
  type: "fuji-pairing";
  sessionId: string;
  deviceId: string;
  deviceName: string;
  ipAddress: string;
  port: number;
  expiresAt: number;
  sunshineOTP: {
    pin: string;
    passphrase: string;
    expiresAt: number;
    urls: {
      http: string;
      https: string;
    };
  };
}
```

---

## Appendix B: Health Check Endpoint

### Request

```http
GET /api/health
```

*No authentication required*

### Response

```json
{
  "status": "ok",
  "version": "1.0.0",
  "uptime_secs": 3600,
  "active_streams": 0,
  "hosts": {
    "total": 1,
    "paired": 1,
    "online": 1
  },
  "remote_access": {
    "upnp_enabled": true,
    "upnp_success": true,
    "external_ip": "73.45.123.89",
    "nat_type": "restricted_cone"
  }
}
```

### Usage in Backlight

```typescript
class WebServerHealthCheck {
  private healthCheckInterval: NodeJS.Timer | null = null;
  private consecutiveFailures = 0;
  private maxFailures = 3;

  start(port: number): void {
    this.healthCheckInterval = setInterval(async () => {
      try {
        const response = await fetch(`http://localhost:${port}/api/health`, {
          timeout: 5000
        });
        
        if (response.ok) {
          this.consecutiveFailures = 0;
          const health = await response.json();
          this.emit('health', health);
        } else {
          this.handleFailure();
        }
      } catch (error) {
        this.handleFailure();
      }
    }, 10000); // Check every 10 seconds
  }

  private handleFailure(): void {
    this.consecutiveFailures++;
    
    if (this.consecutiveFailures >= this.maxFailures) {
      this.emit('unhealthy');
      // Trigger restart logic
    }
  }

  stop(): void {
    if (this.healthCheckInterval) {
      clearInterval(this.healthCheckInterval);
    }
  }
}
```
