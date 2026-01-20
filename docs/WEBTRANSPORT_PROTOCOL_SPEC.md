# WebTransport Protocol Specification

This document defines the complete WebTransport streaming protocol, including connection flow, channel architecture, and fallback logic.

## Overview

The WebTransport implementation provides a complete streaming solution using QUIC/HTTP3, with support for:
- Video streaming via unreliable datagrams
- Audio streaming via reliable unidirectional stream
- Input handling via reliable bidirectional streams
- Automatic fallback to WebRTC when WebTransport is unavailable

## Connection Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLIENT                                          │
│                                                                              │
│  ┌──────────────┐     ┌──────────────────────────────────────────────────┐  │
│  │   WebView    │     │              Native Client                        │  │
│  │              │     │                                                   │  │
│  │  WebSocket   │────▶│  1. Receives Setup via bridge                    │  │
│  │  (signaling) │     │  2. Chooses transport (WT preferred)             │  │
│  │              │     │  3. Connects WebTransport (video/audio)          │  │
│  └──────────────┘     │  4. Connects WebTransport input (with token)     │  │
│                       └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ QUIC/HTTP3
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SERVER                                          │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    WebTransport Endpoint (:4433)                      │   │
│  │                                                                       │   │
│  │   /webtransport           → Main session (video datagrams, audio)    │   │
│  │   /webtransport/input     → Input session (bidirectional streams)    │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    WebRTC Endpoint (fallback)                         │   │
│  │                                                                       │   │
│  │   /host/stream            → WebSocket signaling                       │   │
│  │   /host/input             → Input signaling (hybrid mode)             │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Connection Flow

### Phase 1: Signaling (WebSocket)

```
Client                                    Server
   │                                         │
   │──── WebSocket: /host/stream ───────────▶│
   │                                         │
   │──── Init {                              │
   │       host_id, app_id,                  │
   │       hybrid_mode: true,                │
   │       preferred_transport: "auto"       │  (NEW)
   │     } ─────────────────────────────────▶│
   │                                         │
   │◀──── Setup {                            │
   │        ice_servers: [...],              │  (for WebRTC fallback)
   │        session_token: "abc123",         │
   │        webtransport_url: "https://...", │
   │        cert_hash: "sha256-...",         │
   │        transport_ready: "webtransport"  │  (NEW: indicates which is ready)
   │      } ─────────────────────────────────│
   │                                         │
```

### Phase 2: Transport Selection (Client-Side)

```javascript
// Client decision logic
if (preferred === "webtransport" || preferred === "auto") {
    if (webtransport_url && cert_hash && isWebTransportSupported()) {
        try {
            await connectWebTransport(webtransport_url, cert_hash, session_token);
            return; // Success
        } catch (e) {
            // Fall through to WebRTC
            notifyServer({ FallbackToWebRtc: { reason: e.message } });
        }
    }
}
// Fall back to WebRTC
await connectWebRTC(ice_servers, session_token);
```

### Phase 3A: WebTransport Connection

```
Client                                    Server
   │                                         │
   │  ═══ QUIC Connection ═══════════════════│
   │                                         │
   │──── CONNECT /webtransport ─────────────▶│  Main session
   │◀──── 200 OK ────────────────────────────│
   │                                         │
   │  [Video: Server → Client datagrams]     │
   │  [Audio: Server → Client uni-stream]    │
   │                                         │
   │──── CONNECT /webtransport/input         │  Input session
   │      ?token=abc123 ────────────────────▶│
   │◀──── 200 OK ────────────────────────────│
   │                                         │
   │  [Input: Client → Server bi-streams]    │
   │                                         │
```

### Phase 3B: WebRTC Fallback

```
Client                                    Server
   │                                         │
   │──── { FallbackToWebRtc: {...} } ───────▶│  (via WebSocket)
   │                                         │
   │◀──── { WebRtc: Offer } ─────────────────│  (standard WebRTC flow)
   │──── { WebRtc: Answer } ─────────────────▶│
   │◀───▶ { WebRtc: IceCandidate } ──────────│
   │                                         │
   │  [Video/Audio: WebRTC media tracks]     │
   │                                         │
   │──── /host/input (WebSocket) ───────────▶│  Input (hybrid mode)
   │──── { Join: { token: "abc123" } } ─────▶│
   │◀──── { Accepted: { ice_servers } } ─────│
   │◀───▶ { WebRtc: signaling } ─────────────│
   │                                         │
```

## WebTransport Channel Protocol

### Video (Datagrams)

Video frames are sent as unreliable datagrams from server to client.

**Datagram Format:**
```
┌─────────────────────────────────────────────────────────────┐
│  Header (8 bytes)                                           │
│  ┌─────────┬─────────┬─────────┬───────────────────────────┐│
│  │ Type(1) │ Seq(4)  │ Flags(1)│ Timestamp(2)              ││
│  └─────────┴─────────┴─────────┴───────────────────────────┘│
│  Payload (NAL unit data)                                    │
└─────────────────────────────────────────────────────────────┘

Type:
  0x01 = H264 NAL
  0x02 = H265 NAL  
  0x03 = AV1 OBU

Flags:
  0x01 = Keyframe (IDR)
  0x02 = End of frame
  0x04 = Start of frame
```

### Audio (Unidirectional Stream)

Audio is sent via a reliable unidirectional stream (server → client).

**Stream Format:**
```
┌─────────────────────────────────────────────────────────────┐
│  Header (4 bytes)                                           │
│  ┌─────────────────┬─────────────────────────────────────────┐
│  │ Length(2)       │ Timestamp(2)                           │
│  └─────────────────┴─────────────────────────────────────────┘
│  Payload (Opus audio data)                                  │
└─────────────────────────────────────────────────────────────┘
```

### Input (Bidirectional Streams)

Input uses bidirectional streams, one per channel type. Client creates streams.

**Channel IDs:**
```
0x01 = MOUSE_RELIABLE     (clicks, wheel)
0x02 = MOUSE_RELATIVE     (movement deltas)
0x03 = MOUSE_ABSOLUTE     (touch-to-mouse position)
0x04 = KEYBOARD           (key events)
0x05 = TOUCH              (touch events)
0x06 = CONTROLLERS        (gamepad state)
0x10-0x1F = CONTROLLER_0 through CONTROLLER_15
0x30 = STATS              (latency measurements)
```

**Stream Initialization:**
```
Client creates bidirectional stream
Client sends: [channel_id: u8]
Server acknowledges by accepting the stream
Bidirectional data flow begins
```

**Input Message Format:**
```
┌─────────────────────────────────────────────────────────────┐
│  Length(2) │ Payload (channel-specific data)                │
└─────────────────────────────────────────────────────────────┘
```

## Session Token Validation

For the input session, the token is passed as a query parameter:
```
/webtransport/input?token=<session_token>
```

Server validates:
1. Token matches the active session
2. Main session for this token is connected
3. No existing input session for this token

## Fallback Triggers

The client should fall back to WebRTC when:

1. **WebTransport Not Supported** - Browser/WebView doesn't have WebTransport API
2. **Connection Failed** - QUIC handshake fails (blocked by firewall, etc.)
3. **Certificate Error** - Server cert hash doesn't match
4. **Server Not Ready** - `webtransport_url` not provided in Setup
5. **Timeout** - Connection doesn't establish within 5 seconds

**Fallback Message (Client → Server via WebSocket):**
```json
{
  "FallbackToWebRtc": {
    "reason": "connection_failed",
    "error": "QUIC handshake timeout"
  }
}
```

**Server Response:**
Server proceeds with WebRTC signaling flow.

## Server Configuration

```toml
[webtransport]
# Enable WebTransport (default: true if certificates available)
enabled = true

# Bind address for WebTransport endpoint
bind_address = "0.0.0.0:4433"

# Certificate paths (optional - generates self-signed if not provided)
certificate_path = "/path/to/cert.pem"
private_key_path = "/path/to/key.pem"

# Connection limits
max_concurrent_sessions = 10
session_timeout_secs = 30
```

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 1001 | TOKEN_INVALID | Session token is invalid |
| 1002 | TOKEN_EXPIRED | Session token has expired |
| 1003 | SESSION_NOT_FOUND | No session found for token |
| 1004 | INPUT_ALREADY_CONNECTED | Input session already exists |
| 1005 | MAIN_NOT_CONNECTED | Main session must connect first |
| 1006 | SERVER_FULL | Too many concurrent sessions |

## Stats Protocol

Stats are exchanged on the STATS channel (0x30):

**Client → Server (Input Latency):**
```json
{
  "type": "input_sent",
  "timestamp_ms": 1234567890,
  "seq": 12345
}
```

**Server → Client (Echo for RTT):**
```json
{
  "type": "input_ack", 
  "timestamp_ms": 1234567890,
  "seq": 12345,
  "server_timestamp_ms": 1234567895
}
```
