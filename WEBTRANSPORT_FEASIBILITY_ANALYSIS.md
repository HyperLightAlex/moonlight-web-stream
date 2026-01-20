# WebTransport Implementation Feasibility Analysis
## Moonlight Web Stream - WebTransport Endpoint Investigation

**Date:** January 2025  
**Project:** moonlight-web-stream  
**Objective:** Investigate feasibility and effort for implementing WebTransport as an alternative transport mechanism alongside existing WebRTC implementation

---

## Executive Summary

**Feasibility:** ✅ **HIGH** - WebTransport is technically feasible and well-suited for low-latency game streaming

**Estimated Effort:** **4-6 weeks** for a production-ready implementation supporting both WebRTC and WebTransport

**Key Benefits:**
- Lower latency potential (simpler handshake, no ICE complexity)
- Native support for unreliable datagrams (ideal for video frames)
- Multiple streams on single connection (simplifies hybrid input approach)
- Reduced protocol overhead compared to WebRTC

**Key Challenges:**
- Browser support limitations (primarily Chromium-based browsers)
- TLS certificate requirements (self-signed certs need certificate hash)
- Learning curve for QUIC/HTTP/3 stack
- Testing across different network conditions

---

## 1. Current Architecture Overview

### 1.1 System Components

The moonlight-web-stream project consists of:

1. **Web Server (Rust)** - `moonlight-web/web-server/`
   - HTTP/WebSocket server for signaling
   - User authentication and session management
   - Spawns streamer subprocesses per active stream

2. **Streamer (Rust)** - `moonlight-web/streamer/`
   - WebRTC peer connection management
   - Receives video/audio from Sunshine via Moonlight protocol
   - Transcodes and forwards media over WebRTC
   - Handles input via WebRTC data channels

3. **Web Client (TypeScript)** - `moonlight-web/web-server/web/`
   - WebRTC peer connection (client-side)
   - Video rendering via WebCodecs API
   - Audio playback
   - Input handling (keyboard, mouse, gamepad)

### 1.2 Current WebRTC Flow

```
┌─────────────┐         ┌──────────────┐         ┌─────────────┐
│   Client    │         │  Web Server  │         │  Streamer   │
│  (Browser)  │         │   (Rust)     │         │   (Rust)    │
└──────┬──────┘         └──────┬───────┘         └──────┬──────┘
       │                        │                        │
       │ 1. WebSocket Connect   │                        │
       │────────────────────────►│                        │
       │                        │                        │
       │ 2. Init Stream Request  │                        │
       │────────────────────────►│                        │
       │                        │ 3. Spawn Streamer      │
       │                        │───────────────────────►│
       │                        │                        │
       │ 4. ICE Servers Config  │                        │
       │◄────────────────────────│                        │
       │                        │                        │
       │ 5. WebRTC Offer        │                        │
       │◄────────────────────────│                        │
       │                        │                        │
       │ 6. WebRTC Answer       │                        │
       │────────────────────────►│                        │
       │                        │                        │
       │ 7. ICE Candidates      │                        │
       │◄───────────────────────►│                        │
       │                        │                        │
       │ 8. Media Stream        │                        │
       │◄═══════════════════════════════════════════════►│
       │                        │                        │
```

### 1.3 Hybrid Mode Architecture

The project already implements a "hybrid mode" where input is handled via a separate WebRTC connection:

- **Primary Connection:** Video/Audio via WebRTC (WebView)
- **Input Connection:** Input events via separate WebRTC peer (Native Android client)

This separation improves input latency by bypassing the video jitter buffer.

**Key Files:**
- `moonlight-web/streamer/src/transport/webrtc/mod.rs` - Input peer creation (lines 575-961)
- `moonlight-web/web-server/src/api/input.rs` - Input-only WebSocket endpoint
- `moonlight-web/web-server/web/stream/index.ts` - Hybrid mode handling

---

## 2. WebTransport Overview

### 2.1 What is WebTransport?

WebTransport is a modern web API that provides:
- **Unreliable Datagrams** - UDP-like semantics for low-latency, loss-tolerant data
- **Reliable Streams** - Multiple ordered streams on a single connection
- **Built on QUIC/HTTP/3** - Modern transport protocol with built-in encryption
- **Simpler than WebRTC** - No ICE/STUN/TURN complexity for known endpoints

### 2.2 Browser Support (as of 2025)

| Browser | Support Status | Notes |
|---------|---------------|-------|
| Chrome/Edge | ✅ Full Support | Since Chrome 97 (Jan 2022) |
| Android WebView | ✅ Full Support | Chrome-based |
| Firefox | ⚠️ Experimental | Behind feature flag |
| Safari | ❌ Not Supported | No timeline available |

**Impact:** For Android app use case, support is excellent (Chrome-based WebView).

### 2.3 Advantages for Game Streaming

1. **Lower Latency Potential**
   - No ICE negotiation delay
   - Direct connection establishment
   - Reduced protocol overhead

2. **Unreliable Datagrams**
   - Perfect for video frames (drop late packets)
   - Lower latency than reliable channels
   - Similar to UDP semantics

3. **Multiple Streams**
   - Video on datagrams
   - Audio on separate stream
   - Input on separate stream
   - All on single connection (simplifies hybrid mode)

4. **Simpler Architecture**
   - No SDP offer/answer exchange
   - No ICE candidate gathering
   - Certificate-based authentication

---

## 3. Implementation Approach

### 3.1 High-Level Architecture

```
┌─────────────┐         ┌──────────────┐         ┌─────────────┐
│   Client    │         │  Web Server  │         │  Streamer   │
│  (Browser)  │         │   (Rust)     │         │   (Rust)    │
└──────┬──────┘         └──────┬───────┘         └──────┬──────┘
       │                        │                        │
       │ 1. WebSocket Connect   │                        │
       │────────────────────────►│                        │
       │                        │                        │
       │ 2. Init Stream Request  │                        │
       │────────────────────────►│                        │
       │                        │ 3. Spawn Streamer      │
       │                        │───────────────────────►│
       │                        │                        │
       │ 4. WebTransport URL    │                        │
       │◄────────────────────────│                        │
       │                        │                        │
       │ 5. WebTransport Connect │                        │
       │────────────────────────►│                        │
       │                        │                        │
       │ 6. Media Datagrams     │                        │
       │◄═══════════════════════════════════════════════►│
       │                        │                        │
```

### 3.2 Rust Backend Implementation

#### 3.2.1 Required Crates

**Option 1: wtransport (Recommended)**
```toml
[dependencies]
wtransport = "0.4"  # WebTransport server implementation
```

**Option 2: quinn + custom WebTransport layer**
```toml
[dependencies]
quinn = "0.11"      # QUIC implementation
http = "1.0"       # HTTP/3 support
```

**Recommendation:** Use `wtransport` as it provides a higher-level API specifically for WebTransport.

#### 3.2.2 New Transport Module Structure

```
moonlight-web/streamer/src/transport/
├── mod.rs                    # Transport trait (existing)
├── webrtc/                   # Existing WebRTC implementation
│   └── mod.rs
└── webtransport/            # NEW: WebTransport implementation
    ├── mod.rs               # Main WebTransport transport
    ├── video.rs             # Video datagram sender
    ├── audio.rs             # Audio stream sender
    └── channels.rs          # Data channel equivalents
```

#### 3.2.3 Implementation Pattern

The existing code uses a `TransportSender` trait pattern:

```rust
#[async_trait]
pub trait TransportSender {
    async fn setup_video(&self, setup: VideoSetup) -> i32;
    async fn send_video_unit(&self, unit: &VideoDecodeUnit) -> Result<DecodeResult, TransportError>;
    async fn setup_audio(&self, audio_config: AudioConfig, stream_config: OpusMultistreamConfig) -> i32;
    async fn send_audio_sample(&self, data: &[u8]) -> Result<(), TransportError>;
    async fn send(&self, packet: OutboundPacket) -> Result<(), TransportError>;
    async fn on_ipc_message(&self, message: ServerIpcMessage) -> Result<(), TransportError>;
    async fn close(&self) -> Result<(), TransportError>;
}
```

A new `WebTransportSender` would implement this same trait, allowing drop-in replacement.

#### 3.2.4 Key Implementation Details

**1. Server Setup (in streamer)**
```rust
// Create WebTransport server endpoint
let server_config = ServerConfig::builder()
    .with_bind_address(SocketAddr::from(([0, 0, 0, 0], 4433)))
    .with_certificate(cert_chain, private_key)
    .build();

let server = Endpoint::server(server_config)?;
```

**2. Session Handling**
```rust
// Accept incoming WebTransport sessions
while let Some(connection_request) = server.accept().await {
    let session = connection_request.accept().await?;
    
    // Handle video datagrams
    let datagrams = session.datagrams();
    // Handle audio stream
    let audio_stream = session.accept_uni().await?;
    // Handle input stream
    let input_stream = session.accept_bi().await?;
}
```

**3. Video Frame Sending**
```rust
// Send video frame as unreliable datagram
async fn send_video_frame(&self, frame_data: &[u8]) -> Result<(), TransportError> {
    self.datagrams.send(frame_data).await
        .map_err(|e| TransportError::Implementation(e.into()))
}
```

**4. Audio Sample Sending**
```rust
// Send audio samples on reliable stream
async fn send_audio_sample(&self, data: &[u8]) -> Result<(), TransportError> {
    self.audio_stream.write_all(data).await
        .map_err(|e| TransportError::Implementation(e.into()))
}
```

### 3.3 TypeScript Frontend Implementation

#### 3.3.1 New Transport Class

Create `moonlight-web/web-server/web/stream/transport/webtransport.ts`:

```typescript
export class WebTransportTransport implements Transport {
    implementationName: string = "webtransport"
    
    private transport: WebTransport | null = null
    private datagrams: ReadableStream<Uint8Array> | null = null
    private audioStream: ReadableStream<Uint8Array> | null = null
    private inputStream: WritableStream<Uint8Array> | null = null
    
    async initTransport(url: string, serverCertificateHashes?: Array<{algorithm: string, value: BufferSource}>): Promise<void> {
        const options: WebTransportOptions = {}
        
        if (serverCertificateHashes) {
            options.serverCertificateHashes = serverCertificateHashes
        }
        
        this.transport = new WebTransport(url, options)
        await this.transport.ready
        
        // Get datagrams for video
        this.datagrams = this.transport.datagrams.readable
        
        // Accept audio stream
        const audioReader = this.transport.incomingUnidirectionalStreams.getReader()
        const { value: audioStream } = await audioReader.read()
        this.audioStream = audioStream
        
        // Create bidirectional stream for input
        this.inputStream = await this.transport.createBidirectionalStream()
    }
    
    // Implement Transport interface methods...
}
```

#### 3.3.2 Integration with Existing Stream Class

Modify `moonlight-web/web-server/web/stream/index.ts`:

```typescript
private async tryWebTransportTransport() {
    this.debugLog("Trying WebTransport transport")
    
    if (!this.webTransportUrl) {
        this.debugLog("Failed to try WebTransport: no URL available")
        return
    }
    
    const transport = new WebTransportTransport(this.logger)
    
    // Get certificate hash from server if needed
    const certHash = await this.getWebTransportCertHash()
    
    await transport.initTransport(this.webTransportUrl, certHash ? [certHash] : undefined)
    this.setTransport(transport)
}
```

### 3.4 Server-Side Changes

#### 3.4.1 Web Server Updates

**File:** `moonlight-web/web-server/src/api/stream.rs`

Add WebTransport URL to Setup message:

```rust
StreamServerMessage::Setup {
    ice_servers: config.ice_servers.clone(),
    session_token,
    webtransport_url: Some(format!("https://{}:{}/webtransport", host, port)),
    webtransport_cert_hash: Some(certificate_hash), // For self-signed certs
}
```

#### 3.4.2 Streamer Updates

**File:** `moonlight-web/streamer/src/main.rs`

Add transport selection logic:

```rust
let (sender, mut events) = match config.transport_type {
    TransportType::WebRTC => {
        webrtc::new(settings.clone(), &config.webrtc, session_token).await?
    }
    TransportType::WebTransport => {
        webtransport::new(settings.clone(), &config.webtransport, session_token).await?
    }
}
```

### 3.5 Configuration

Add WebTransport config to `moonlight-web/common/src/config.rs`:

```rust
pub struct WebTransportConfig {
    pub bind_address: Option<String>,
    pub certificate_path: Option<String>,
    pub private_key_path: Option<String>,
    pub certificate_hash: Option<String>, // For client-side validation
}
```

---

## 4. Level of Effort Breakdown

### 4.1 Backend (Rust) - 2-3 weeks

| Task | Effort | Complexity |
|------|--------|------------|
| Research wtransport/quinn crates | 4h | Low |
| Create WebTransport transport module | 16h | Medium |
| Implement video datagram sender | 8h | Medium |
| Implement audio stream sender | 8h | Medium |
| Implement data channel equivalents | 12h | Medium |
| Integrate with existing transport trait | 8h | Low |
| Add WebTransport config support | 4h | Low |
| Certificate handling (self-signed) | 8h | Medium |
| Testing and debugging | 16h | High |
| **Total** | **84h (~2.1 weeks)** | |

### 4.2 Frontend (TypeScript) - 1-2 weeks

| Task | Effort | Complexity |
|------|--------|------------|
| Research WebTransport API | 4h | Low |
| Create WebTransportTransport class | 12h | Medium |
| Implement video datagram receiver | 8h | Medium |
| Implement audio stream receiver | 8h | Medium |
| Implement input stream sender | 8h | Medium |
| Integrate with existing Stream class | 8h | Low |
| Certificate hash handling | 4h | Low |
| Fallback to WebRTC logic | 4h | Low |
| Testing and debugging | 12h | High |
| **Total** | **68h (~1.7 weeks)** | |

### 4.3 Server Integration - 1 week

| Task | Effort | Complexity |
|------|--------|------------|
| Add WebTransport URL to Setup message | 4h | Low |
| Add certificate hash generation | 4h | Medium |
| Update streamer spawn logic | 4h | Low |
| Add WebTransport config parsing | 4h | Low |
| Update API bindings (TypeScript) | 4h | Low |
| Testing | 8h | Medium |
| **Total** | **28h (~0.7 weeks)** | |

### 4.4 Testing & Documentation - 1 week

| Task | Effort | Complexity |
|------|--------|------------|
| Unit tests (Rust) | 8h | Medium |
| Integration tests | 12h | High |
| Browser compatibility testing | 8h | Medium |
| Network condition testing | 8h | Medium |
| Performance benchmarking | 8h | Medium |
| Documentation | 8h | Low |
| **Total** | **52h (~1.3 weeks)** | |

### 4.5 Total Estimated Effort

**Conservative Estimate:** 6 weeks (232 hours)  
**Optimistic Estimate:** 4 weeks (160 hours)  
**Realistic Estimate:** 5 weeks (190 hours)

---

## 5. Technical Challenges & Solutions

### 5.1 Certificate Management

**Challenge:** WebTransport requires TLS certificates. For local development/testing, self-signed certs need special handling.

**Solution:**
- Use certificate fingerprint/hash for client-side validation
- Generate self-signed certs automatically on first run
- Provide config option for production certificates
- Document certificate setup process

**Implementation:**
```rust
// Generate certificate hash for client validation
let cert_hash = calculate_certificate_hash(&certificate);
// Send to client in Setup message
```

### 5.2 Browser Compatibility

**Challenge:** WebTransport not supported in all browsers.

**Solution:**
- Implement feature detection
- Fallback to WebRTC if WebTransport unavailable
- Provide user-facing error messages
- Document browser requirements

**Implementation:**
```typescript
if (!('WebTransport' in window)) {
    console.warn('WebTransport not supported, falling back to WebRTC')
    await this.tryWebRTCTransport()
    return
}
```

### 5.3 Hybrid Mode Simplification

**Challenge:** Current hybrid mode uses separate WebRTC connection for input.

**Solution:**
- Use WebTransport bidirectional streams for input
- Single connection for video, audio, and input
- Simplify architecture by removing separate input peer

**Benefit:** This is actually simpler than current WebRTC hybrid approach!

### 5.4 Network Path Discovery

**Challenge:** WebRTC uses ICE for NAT traversal. WebTransport needs alternative approach.

**Solution:**
- For known server IPs, direct connection
- For remote access, use existing STUN/UPnP infrastructure
- Consider TURN server for WebTransport (future work)

---

## 6. Comparison: WebRTC vs WebTransport

| Feature | WebRTC (Current) | WebTransport (Proposed) |
|---------|------------------|-------------------------|
| **Latency** | Low (~50-100ms) | Potentially Lower (~30-80ms) |
| **Setup Complexity** | High (ICE/SDP) | Low (Direct connection) |
| **Protocol Overhead** | Higher (RTP/SRTP) | Lower (QUIC native) |
| **Unreliable Transport** | Via data channels | Native datagrams |
| **Multiple Streams** | Multiple data channels | Native streams |
| **Browser Support** | Universal | Chromium-based |
| **NAT Traversal** | ICE/STUN/TURN | Direct (known IP) or TURN |
| **Certificate Requirements** | Optional | Required (TLS) |
| **Hybrid Mode** | Separate peer | Single connection |

---

## 7. Implementation Recommendations

### 7.1 Phased Approach

**Phase 1: Proof of Concept (Week 1-2)**
- Implement basic WebTransport server in Rust
- Implement basic WebTransport client in TypeScript
- Test video-only streaming
- Validate latency improvements

**Phase 2: Full Feature Implementation (Week 3-4)**
- Add audio streaming
- Add input handling
- Integrate with existing transport system
- Add configuration support

**Phase 3: Testing & Polish (Week 5-6)**
- Comprehensive testing
- Performance benchmarking
- Documentation
- Browser compatibility validation

### 7.2 Code Organization

**Maintain Parallel Implementations:**
- Keep WebRTC implementation intact
- Add WebTransport as alternative
- Use factory pattern for transport selection
- Allow runtime switching (for testing)

**File Structure:**
```
moonlight-web/streamer/src/transport/
├── mod.rs                    # Transport trait + factory
├── webrtc/                   # Existing (unchanged)
└── webtransport/            # New implementation
```

### 7.3 Configuration Strategy

**Add transport selection to config:**
```json
{
  "transport": {
    "type": "auto",  // "auto", "webrtc", "webtransport"
    "webrtc": { ... },
    "webtransport": {
      "bind_address": "0.0.0.0:4433",
      "certificate_path": "./certs/cert.pem",
      "private_key_path": "./certs/key.pem"
    }
  }
}
```

**Auto-detection logic:**
1. Try WebTransport if supported
2. Fallback to WebRTC
3. Log transport selection for debugging

---

## 8. Risk Assessment

### 8.1 Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Browser compatibility issues | Medium | High | Feature detection + fallback |
| Certificate management complexity | Medium | Medium | Auto-generation + documentation |
| Performance not better than WebRTC | Low | Medium | Benchmark early, keep WebRTC |
| QUIC/HTTP/3 learning curve | Medium | Low | Use high-level library (wtransport) |
| Network traversal issues | Medium | High | Use existing STUN infrastructure |

### 8.2 Project Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Timeline overrun | Medium | Medium | Phased approach, prioritize core features |
| Integration complexity | Low | Medium | Maintain parallel implementations |
| Testing coverage | Medium | High | Allocate dedicated testing time |

---

## 9. Success Criteria

### 9.1 Functional Requirements

- ✅ Video streaming works via WebTransport
- ✅ Audio streaming works via WebTransport
- ✅ Input handling works via WebTransport
- ✅ Fallback to WebRTC if WebTransport unavailable
- ✅ Hybrid mode works (single connection for all)

### 9.2 Performance Requirements

- Latency: ≤ 80ms end-to-end (target: 50ms)
- Frame rate: Maintain 60fps at target resolution
- Packet loss: Handle gracefully (drop late frames)
- CPU usage: Comparable to WebRTC

### 9.3 Quality Requirements

- Code coverage: ≥ 80% for new code
- Documentation: Complete API docs
- Browser support: Chrome/Edge/Android WebView
- Error handling: Graceful degradation

---

## 10. Next Steps

### 10.1 Immediate Actions

1. **Research & Prototype (Week 1)**
   - Set up wtransport test server
   - Create minimal WebTransport client
   - Measure baseline latency

2. **Design Review (Week 1)**
   - Review architecture with team
   - Validate approach with stakeholders
   - Get approval to proceed

3. **Implementation Planning (Week 1)**
   - Create detailed task breakdown
   - Set up development environment
   - Create feature branch

### 10.2 Decision Points

**Week 2 Checkpoint:**
- Evaluate POC results
- Compare latency vs WebRTC
- Decide on full implementation

**Week 4 Checkpoint:**
- Review integration progress
- Assess remaining work
- Adjust timeline if needed

---

## 11. Conclusion

WebTransport is a **highly feasible** alternative to WebRTC for game streaming, with potential benefits in latency and architectural simplicity. The implementation effort is **moderate** (4-6 weeks) and can be done alongside the existing WebRTC implementation.

**Key Advantages:**
- Lower latency potential
- Simpler architecture (especially for hybrid mode)
- Native unreliable datagrams for video
- Multiple streams on single connection

**Key Considerations:**
- Browser support (primarily Chromium-based)
- Certificate management requirements
- Learning curve for QUIC/HTTP/3
- Need for comprehensive testing

**Recommendation:** ✅ **Proceed with implementation** in a phased approach, maintaining WebRTC as fallback option.

---

## Appendix A: Reference Implementation Examples

### A.1 Rust WebTransport Server (wtransport)

```rust
use wtransport::{ServerConfig, Endpoint, ServerBuilder};

async fn create_webtransport_server() -> Result<Endpoint, Error> {
    let config = ServerConfig::builder()
        .with_bind_address(SocketAddr::from(([0, 0, 0, 0], 4433)))
        .with_certificate(cert_chain, private_key)
        .build();
    
    let server = Endpoint::server(config)?;
    Ok(server)
}
```

### A.2 TypeScript WebTransport Client

```typescript
const transport = new WebTransport('https://server:4433/webtransport', {
    serverCertificateHashes: [{
        algorithm: 'sha-256',
        value: certHashBuffer
    }]
});

await transport.ready;

// Receive video datagrams
const datagrams = transport.datagrams.readable;
const reader = datagrams.getReader();

while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    // Decode video frame
    decodeVideoFrame(value);
}
```

---

## Appendix B: Useful Resources

- [W3C WebTransport Specification](https://www.w3.org/TR/webtransport/)
- [wtransport Rust Crate](https://docs.rs/wtransport/)
- [MDN WebTransport API](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
- [Chrome WebTransport Guide](https://web.dev/webtransport/)
- [QUIC Protocol Specification](https://datatracker.ietf.org/doc/html/rfc9000)

---

**Document Version:** 1.0  
**Last Updated:** January 2025  
**Author:** AI Assistant (based on codebase analysis)
