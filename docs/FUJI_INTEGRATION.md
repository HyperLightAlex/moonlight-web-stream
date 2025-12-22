# Fuji Integration Guide for moonlight-web-stream

This document describes how moonlight-web-stream integrates with Fuji Desktop for automatic OTP-based pairing, enabling seamless streaming setup for Backbone app users.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Host Discovery](#host-discovery)
4. [Pairing Flow](#pairing-flow)
5. [API Reference](#api-reference)
6. [TypeScript Types](#typescript-types)
7. [Backbone App Integration](#backbone-app-integration)
8. [Error Handling](#error-handling)
9. [Configuration](#configuration)
10. [Troubleshooting](#troubleshooting)

---

## Overview

### What is Fuji?

Fuji Desktop is an Electron-based wrapper for Sunshine that provides:
- Automatic game scanning and library management
- QR code pairing for mobile devices
- Enhanced user experience over Sunshine's default web UI
- Bundled Sunshine instance with pre-configured settings

### What is moonlight-web-stream?

moonlight-web-stream is a web-based Moonlight client that:
- Allows streaming games via WebRTC from any browser
- Manages multiple host connections
- Provides user authentication and multi-user support
- Works with both standard Sunshine and Fuji hosts

### Integration Goal

Enable automatic pairing between moonlight-web-stream and Fuji hosts without requiring manual PIN entry, leveraging Fuji's OTP (One-Time Password) pairing mechanism.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER DEVICES                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌──────────────┐         ┌──────────────┐         ┌──────────────┐        │
│   │  Backbone    │         │   Browser    │         │   Other      │        │
│   │  Mobile App  │         │   (Web UI)   │         │   Clients    │        │
│   └──────┬───────┘         └──────┬───────┘         └──────┬───────┘        │
│          │                        │                        │                 │
└──────────┼────────────────────────┼────────────────────────┼─────────────────┘
           │                        │                        │
           │         HTTPS/WSS      │                        │
           └────────────────────────┼────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        MOONLIGHT-WEB-STREAM SERVER                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌────────────────┐    ┌────────────────┐    ┌────────────────┐            │
│   │  Auth Module   │    │  Host Manager  │    │  Stream Relay  │            │
│   │  (User Auth)   │    │  (Pair/Detect) │    │  (WebRTC)      │            │
│   └────────────────┘    └───────┬────────┘    └────────────────┘            │
│                                 │                                            │
└─────────────────────────────────┼────────────────────────────────────────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    │                           │
                    ▼                           ▼
┌─────────────────────────────┐   ┌─────────────────────────────┐
│      FUJI HOST              │   │   STANDARD SUNSHINE HOST    │
├─────────────────────────────┤   ├─────────────────────────────┤
│                             │   │                             │
│  ┌───────────────────────┐  │   │  ┌───────────────────────┐  │
│  │   Fuji Desktop App    │  │   │  │   Sunshine Server     │  │
│  │   (Electron)          │  │   │  │                       │  │
│  └───────────┬───────────┘  │   │  │   HTTP:  47989        │  │
│              │              │   │  │   HTTPS: 47990        │  │
│  ┌───────────▼───────────┐  │   │  │                       │  │
│  │  Bundled Sunshine     │  │   │  └───────────────────────┘  │
│  │  (Fuji Fork)          │  │   │                             │
│  │                       │  │   │  Pairing: Manual PIN        │
│  │  HTTP:  48989         │  │   │                             │
│  │  HTTPS: 48990         │  │   └─────────────────────────────┘
│  │  OTP Endpoint: ✓      │  │
│  │                       │  │
│  └───────────────────────┘  │
│                             │
│  Pairing: Automatic OTP     │
│                             │
└─────────────────────────────┘
```

### Key Differences: Fuji vs Standard Sunshine

| Feature | Fuji Host | Standard Sunshine |
|---------|-----------|-------------------|
| Default HTTP Port | 48989 | 47989 |
| Default HTTPS Port | 48990 | 47990 |
| OTP Endpoint | ✅ Available | ❌ Not Available |
| Auto-Pairing | ✅ Supported | ❌ Manual PIN Only |
| Web UI | ❌ Disabled | ✅ Available |
| API Auth | Basic Auth (username:password) | Basic Auth (configurable) |

---

## Host Discovery

### Adding a Host

Hosts are added to moonlight-web-stream via the API:

```http
POST /api/host
Content-Type: application/json

{
  "address": "192.168.1.100",
  "http_port": 48989
}
```

**Response:**
```json
{
  "host": {
    "host_id": 1,
    "name": "Gaming PC",
    "paired": "NotPaired",
    "server_state": "Free",
    "address": "192.168.1.100",
    "http_port": 48989,
    "https_port": 48990,
    ...
  }
}
```

### Listing Hosts

```http
GET /api/hosts
```

**Response (streaming NDJSON):**
```json
{"hosts": [{"host_id": 1, "name": "Gaming PC", "paired": "NotPaired", ...}]}
{"host_id": 1, "name": "Gaming PC", "paired": "NotPaired", "server_state": "Free", ...}
```

### Host Detection

When pairing is initiated, the server automatically detects the host type:

1. **Fuji Detection**: Server attempts to access `/otp/request` endpoint
   - Success (HTTP 200) → Fuji host
   - Failure (HTTP 404 or connection error) → Standard Sunshine

2. **Detection Endpoint** (internal):
   ```
   GET https://{host}:{https_port}/otp/request?passphrase=test&deviceName=detection
   Authorization: Basic dXNlcm5hbWU6cGFzc3dvcmQ=
   ```

---

## Pairing Flow

### Flow Diagram

```
┌─────────────┐     ┌─────────────────┐     ┌─────────────┐
│  Client App │     │ moonlight-web   │     │  Host       │
│  (Backbone) │     │    -stream      │     │ (Fuji/Sun)  │
└──────┬──────┘     └────────┬────────┘     └──────┬──────┘
       │                     │                     │
       │  POST /api/pair     │                     │
       │  {host_id: 1}       │                     │
       │────────────────────>│                     │
       │                     │                     │
       │                     │  Detect Host Type   │
       │                     │  (probe OTP endpoint)
       │                     │────────────────────>│
       │                     │                     │
       │                     │<────────────────────│
       │                     │  (200 OK = Fuji)    │
       │                     │  (404 = Standard)   │
       │                     │                     │
       │                     │                     │
   ┌───┴─────────────────────┴─────────────────────┴───┐
   │              IF FUJI HOST (Auto-Pair)             │
   └───┬─────────────────────┬─────────────────────┬───┘
       │                     │                     │
       │  {FujiAutoPairing}  │                     │
       │<────────────────────│                     │
       │                     │                     │
       │  (show loading UI)  │  Request OTP        │
       │                     │────────────────────>│
       │                     │                     │
       │                     │  {pin, expiresAt}   │
       │                     │<────────────────────│
       │                     │                     │
       │                     │  Standard Pair Flow │
       │                     │  (using OTP as PIN) │
       │                     │<------------------->│
       │                     │                     │
       │  {Paired: host}     │                     │
       │<────────────────────│                     │
       │                     │                     │
   ┌───┴─────────────────────┴─────────────────────┴───┐
   │           IF STANDARD SUNSHINE (Manual PIN)       │
   └───┬─────────────────────┬─────────────────────┬───┘
       │                     │                     │
       │  {Pin: "1234"}      │                     │
       │<────────────────────│                     │
       │                     │                     │
       │  (show PIN to user) │                     │
       │                     │                     │
       │  User enters PIN    │  Standard Pair Flow │
       │  on Sunshine Web UI │<------------------->│
       │                     │                     │
       │  {Paired: host}     │                     │
       │<────────────────────│                     │
       │                     │                     │
       ▼                     ▼                     ▼
```

### Fuji OTP Request Details

**Endpoint:** `GET /otp/request`

**Query Parameters:**
| Parameter | Description |
|-----------|-------------|
| `passphrase` | Unique identifier for this pairing session (UUID recommended) |
| `deviceName` | Name to identify this client on the host |

**Headers:**
```http
Authorization: Basic dXNlcm5hbWU6cGFzc3dvcmQ=
```
(Base64 encoded `username:password` - Fuji's default credentials)

**Response:**
```json
{
  "pin": "1234",
  "expiresAt": 1703275200000
}
```

---

## API Reference

### POST /api/pair

Initiate pairing with a host.

**Request:**
```http
POST /api/pair
Content-Type: application/json
Authorization: Bearer {session_token}

{
  "host_id": 1
}
```

**Response (Streaming NDJSON):**

The response is sent as newline-delimited JSON with two stages:

**Stage 1 - Pairing Type (immediate):**

For Fuji hosts:
```json
"FujiAutoPairing"
```

For Standard Sunshine:
```json
{"Pin": "1234"}
```

**Stage 2 - Pairing Result (after pairing completes):**

Success:
```json
{
  "Paired": {
    "host_id": 1,
    "name": "Gaming PC",
    "paired": "Paired",
    "server_state": "Free",
    "address": "192.168.1.100",
    "http_port": 48989,
    "https_port": 48990,
    ...
  }
}
```

Failure:
```json
"PairError"
```

### GET /api/hosts

List all hosts for the authenticated user.

**Request:**
```http
GET /api/hosts
Authorization: Bearer {session_token}
```

**Response (Streaming NDJSON):**
```json
{"hosts": [{"host_id": 1, "name": "Gaming PC", "paired": "NotPaired", "owner": "ThisUser", "server_state": null}]}
{"host_id": 1, "name": "Gaming PC", "paired": "NotPaired", "owner": "ThisUser", "server_state": "Free"}
```

### GET /api/host

Get detailed information about a specific host.

**Request:**
```http
GET /api/host?host_id=1
Authorization: Bearer {session_token}
```

**Response:**
```json
{
  "host": {
    "host_id": 1,
    "owner": "ThisUser",
    "name": "Gaming PC",
    "paired": "Paired",
    "server_state": "Free",
    "address": "192.168.1.100",
    "http_port": 48989,
    "https_port": 48990,
    "external_port": 47998,
    "version": "7.0.0",
    "gfe_version": "3.0.0",
    "unique_id": "ABC123",
    "mac": "AA:BB:CC:DD:EE:FF",
    "local_ip": "192.168.1.100",
    "current_game": 0,
    "max_luma_pixels_hevc": 0,
    "server_codec_mode_support": 0
  }
}
```

### POST /api/host

Add a new host.

**Request:**
```http
POST /api/host
Content-Type: application/json
Authorization: Bearer {session_token}

{
  "address": "192.168.1.100",
  "http_port": 48989
}
```

### GET /api/apps

Get list of applications/games from a paired host.

**Request:**
```http
GET /api/apps?host_id=1
Authorization: Bearer {session_token}
```

**Response:**
```json
{
  "apps": [
    {"app_id": 1, "title": "Desktop", "is_hdr_supported": false},
    {"app_id": 2, "title": "Cyberpunk 2077", "is_hdr_supported": true}
  ]
}
```

---

## TypeScript Types

```typescript
// Host Types
type HostType = "Standard" | "Fuji";

type PairStatus = "NotPaired" | "Paired";

type HostState = "Free" | "Busy";

type HostOwner = "ThisUser" | "Global";

interface UndetailedHost {
  host_id: number;
  owner: HostOwner;
  name: string;
  paired: PairStatus;
  server_state: HostState | null;  // null = offline
}

interface DetailedHost {
  host_id: number;
  owner: HostOwner;
  name: string;
  paired: PairStatus;
  server_state: HostState | null;
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

// Pairing Types
interface PostPairRequest {
  host_id: number;
}

type PostPairResponse1 = 
  | "InternalServerError"
  | "PairError"
  | { Pin: string }           // Standard Sunshine - show PIN
  | "FujiAutoPairing";        // Fuji - auto-pairing in progress

type PostPairResponse2 = 
  | "PairError"
  | { Paired: DetailedHost };

// App Types
interface App {
  app_id: number;
  title: string;
  is_hdr_supported: boolean;
}
```

---

## Backbone App Integration

### Complete Pairing Implementation

```typescript
interface PairResult {
  success: boolean;
  host?: DetailedHost;
  error?: string;
}

async function pairHost(
  apiBaseUrl: string, 
  authToken: string, 
  hostId: number,
  onStatusUpdate: (status: string) => void
): Promise<PairResult> {
  try {
    const response = await fetch(`${apiBaseUrl}/api/pair`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${authToken}`
      },
      body: JSON.stringify({ host_id: hostId })
    });

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }

    const reader = response.body!.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    // Read Stage 1 - Pairing Type
    const stage1 = await readNextJson(reader, decoder, buffer);
    buffer = stage1.remaining;

    if (stage1.data === 'FujiAutoPairing') {
      // Fuji host - automatic pairing
      onStatusUpdate('Auto-pairing with Fuji host...');
    } else if (stage1.data === 'InternalServerError') {
      return { success: false, error: 'Internal server error' };
    } else if (stage1.data === 'PairError') {
      return { success: false, error: 'Pairing failed' };
    } else if (stage1.data.Pin) {
      // Standard Sunshine - manual PIN required
      onStatusUpdate(`Enter PIN on Sunshine: ${stage1.data.Pin}`);
    }

    // Read Stage 2 - Pairing Result
    const stage2 = await readNextJson(reader, decoder, buffer);

    if (stage2.data === 'PairError') {
      return { success: false, error: 'Pairing failed' };
    } else if (stage2.data.Paired) {
      onStatusUpdate('Pairing successful!');
      return { success: true, host: stage2.data.Paired };
    }

    return { success: false, error: 'Unexpected response' };

  } catch (error) {
    return { success: false, error: String(error) };
  }
}

// Helper function to read NDJSON
async function readNextJson(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  decoder: TextDecoder,
  buffer: string
): Promise<{ data: any; remaining: string }> {
  while (true) {
    const newlineIndex = buffer.indexOf('\n');
    if (newlineIndex !== -1) {
      const line = buffer.slice(0, newlineIndex);
      const remaining = buffer.slice(newlineIndex + 1);
      return { data: JSON.parse(line), remaining };
    }

    const { done, value } = await reader.read();
    if (done) {
      // End of stream, parse remaining buffer
      if (buffer.trim()) {
        return { data: JSON.parse(buffer), remaining: '' };
      }
      throw new Error('Stream ended unexpectedly');
    }

    buffer += decoder.decode(value, { stream: true });
  }
}
```

### Usage Example

```typescript
// When user taps on an unpaired host
async function onHostSelected(host: UndetailedHost) {
  if (host.paired === 'Paired') {
    // Already paired, show games
    navigateToGames(host.host_id);
    return;
  }

  // Show pairing UI
  showPairingModal();

  const result = await pairHost(
    API_BASE_URL,
    userAuthToken,
    host.host_id,
    (status) => updatePairingStatus(status)
  );

  if (result.success) {
    hidePairingModal();
    showSuccessToast('Host paired successfully!');
    navigateToGames(host.host_id);
  } else {
    showErrorModal(`Pairing failed: ${result.error}`);
  }
}
```

### UI States

| State | Fuji Host | Standard Sunshine |
|-------|-----------|-------------------|
| Initial | "Auto-pairing with Fuji host..." | "Enter PIN on Sunshine: 1234" |
| In Progress | Loading spinner | Waiting for user action |
| Success | "Pairing successful!" | "Pairing successful!" |
| Error | Error message | Error message |

---

## Error Handling

### Common Errors

| Error | Cause | Resolution |
|-------|-------|------------|
| Host offline | Host unreachable | Check host is running and network connected |
| OTP request failed | Fuji Sunshine not running | Ensure Fuji app is running on host |
| Invalid credentials | Wrong Fuji credentials | Fuji uses default `username:password` |
| Pairing timeout | Host didn't respond | Retry pairing |
| Already paired | Host already paired | No action needed |

### Error Response Codes

| HTTP Status | Meaning |
|-------------|---------|
| 200 | Success |
| 401 | Unauthorized - invalid or expired session |
| 403 | Forbidden - user doesn't have access to host |
| 404 | Host not found |
| 504 | Gateway timeout - host offline |
| 500 | Internal server error |

---

## Configuration

### moonlight-web-stream Server

No special configuration needed for Fuji support. The server automatically:
- Detects Fuji hosts by probing the OTP endpoint
- Uses default Fuji credentials (`username:password`)
- Falls back to standard PIN pairing for non-Fuji hosts

### Fuji Host Requirements

1. **Fuji Desktop must be running** on the host PC
2. **Bundled Sunshine must be active** (Fuji starts it automatically)
3. **Network ports must be accessible:**
   - HTTP: 48989 (or configured port)
   - HTTPS: 48990 (or configured port)

### Adding Fuji Hosts

When adding a Fuji host, use the Fuji-specific ports:

```json
{
  "address": "192.168.1.100",
  "http_port": 48989
}
```

Standard Sunshine hosts use:
```json
{
  "address": "192.168.1.100",
  "http_port": 47989
}
```

---

## Troubleshooting

### Fuji Host Not Detected as Fuji

**Symptoms:** Host shows PIN for manual pairing instead of auto-pairing

**Checks:**
1. Verify Fuji is running on the host
2. Check HTTP port is 48989 (Fuji's default)
3. Verify network connectivity to HTTPS port 48990
4. Check Fuji logs for OTP endpoint errors

### Auto-Pairing Fails

**Symptoms:** "FujiAutoPairing" shown but pairing fails

**Checks:**
1. Ensure Sunshine is running within Fuji
2. Check Fuji hasn't changed default credentials
3. Verify OTP hasn't expired (5-minute window)
4. Check server logs for detailed error

### Host Shows as Offline

**Symptoms:** `server_state: null` in host response

**Checks:**
1. Host PC is powered on
2. Fuji/Sunshine is running
3. Firewall allows connections on ports 48989/48990
4. Correct IP address configured

### Testing OTP Endpoint Directly

```bash
# Test if Fuji OTP endpoint is accessible
curl -k -u "username:password" \
  "https://HOST_IP:48990/otp/request?passphrase=test&deviceName=test"

# Expected response for Fuji:
# {"pin":"1234","expiresAt":1703275200000}

# Expected response for standard Sunshine:
# 404 Not Found
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2024-12-22 | Initial Fuji integration with OTP auto-pairing |

---

## Related Documentation

- [Moonlight Protocol Documentation](https://github.com/moonlight-stream/moonlight-docs)
- [Sunshine API Documentation](https://docs.lizardbyte.dev/projects/sunshine)
- [Fuji Desktop Repository](https://github.com/Backbone-Labs/Fuji-Sunshine)

