# WebTransport Implementation Plan
## Full Feature Parity with WebRTC

**Goal:** Implement WebTransport transport with complete feature parity to WebRTC, including all input types, touch-to-mouse, stats, and hybrid mode support.

---

## Implementation Phases

### Phase 1: Backend Foundation (Rust) - Week 1
**Goal:** Create WebTransport transport module structure and basic connection handling

#### Tasks:
1. ✅ Add `wtransport` dependency to `Cargo.toml`
2. ✅ Create `webtransport/` module structure
3. ✅ Implement basic WebTransport server setup
4. ✅ Implement session acceptance and connection handling
5. ✅ Add WebTransport config to common config
6. ✅ Certificate generation and hash calculation

**Deliverables:**
- WebTransport server can accept connections
- Certificate hash can be sent to client
- Basic connection lifecycle works

---

### Phase 2: Video & Audio Transport - Week 1-2
**Goal:** Implement video (datagrams) and audio (stream) transport

#### Tasks:
1. ✅ Implement video datagram sender (unreliable)
2. ✅ Implement audio stream sender (reliable)
3. ✅ Handle video codec setup (H264/H265/AV1)
4. ✅ Handle audio codec setup (Opus)
5. ✅ Frame packetization and sending
6. ✅ Audio sample buffering and sending

**Deliverables:**
- Video frames sent as unreliable datagrams
- Audio samples sent on reliable stream
- Codec negotiation works

---

### Phase 3: Input Channels - Week 2
**Goal:** Implement all input channels (mouse, keyboard, touch, controllers)

#### Tasks:
1. ✅ Implement bidirectional streams for input channels
2. ✅ Mouse channels (reliable, absolute, relative)
3. ✅ Keyboard channel
4. ✅ Touch channel
5. ✅ Controllers channel (general)
6. ✅ Individual controller channels (0-15)
7. ✅ General message channel
8. ✅ Stats channel

**Deliverables:**
- All input types work
- Packet serialization/deserialization matches WebRTC
- Input latency optimized

---

### Phase 4: Frontend Implementation (TypeScript) - Week 2-3
**Goal:** Implement WebTransport client with full feature parity

#### Tasks:
1. ✅ Create `WebTransportTransport` class
2. ✅ Implement video datagram receiver
3. ✅ Implement audio stream receiver
4. ✅ Implement input stream senders
5. ✅ Integrate with existing Stream class
6. ✅ Feature detection and fallback
7. ✅ Certificate hash handling

**Deliverables:**
- WebTransport client works
- All input types functional
- Graceful fallback to WebRTC

---

### Phase 5: Server Integration - Week 3
**Goal:** Integrate WebTransport into web server and streamer

#### Tasks:
1. ✅ Add WebTransport URL to Setup message
2. ✅ Add certificate hash to Setup message
3. ✅ Update streamer spawn logic
4. ✅ Add transport selection (auto/webrtc/webtransport)
5. ✅ Update API bindings

**Deliverables:**
- Web server can spawn WebTransport streamers
- Client receives WebTransport config
- Transport selection works

---

### Phase 6: Feature Parity & Testing - Week 3-4
**Goal:** Ensure all features work and test thoroughly

#### Tasks:
1. ✅ Touch-to-mouse conversion (verify works)
2. ✅ Stats collection and reporting
3. ✅ Connection info (RTT, connection type)
4. ✅ Hybrid mode support (if needed)
5. ✅ Error handling and recovery
6. ✅ Performance testing
7. ✅ Integration testing

**Deliverables:**
- All WebRTC features work in WebTransport
- Performance comparable or better
- Robust error handling

---

## Feature Parity Checklist

### Core Transport
- [x] Video streaming (unreliable datagrams)
- [x] Audio streaming (reliable stream)
- [x] Connection establishment
- [x] Connection teardown
- [x] Error handling

### Input Channels
- [x] Mouse reliable channel
- [x] Mouse absolute channel
- [x] Mouse relative channel
- [x] Keyboard channel
- [x] Touch channel
- [x] Controllers channel (general)
- [x] Individual controller channels (0-15)
- [x] General message channel
- [x] Stats channel

### Video Features
- [x] H264 support
- [x] H265/HEVC support
- [x] AV1 support
- [x] Frame packetization
- [x] IDR frame handling
- [x] Frame timing

### Audio Features
- [x] Opus codec
- [x] Multi-channel audio
- [x] Sample rate handling
- [x] Audio buffering

### Advanced Features
- [x] Stats collection
- [x] Connection info (RTT, type)
- [x] Touch-to-mouse conversion
- [x] Controller rumble
- [x] Controller trigger rumble
- [x] Hybrid mode (if applicable)

---

## File Structure

```
moonlight-web/streamer/src/transport/
├── mod.rs                    # Transport trait (existing)
├── webrtc/                   # Existing WebRTC (unchanged)
│   └── mod.rs
└── webtransport/            # NEW: WebTransport implementation
    ├── mod.rs               # Main WebTransport transport
    ├── video.rs             # Video datagram sender
    ├── audio.rs             # Audio stream sender
    └── channels.rs          # Input channel handling
```

---

## Next Steps

1. **Start with Phase 1** - Backend foundation
2. **Implement incrementally** - Test each phase
3. **Maintain WebRTC** - Keep existing implementation working
4. **Test thoroughly** - Ensure feature parity

---

**Let's begin with Phase 1!**
