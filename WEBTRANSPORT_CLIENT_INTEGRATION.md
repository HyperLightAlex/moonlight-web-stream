# WebTransport Client Integration Guide

This guide documents how to integrate WebTransport streaming into your Android client using the moonlight-web-stream server.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ANDROID CLIENT                                     │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                         WebView Component                             │   │
│  │                                                                       │   │
│  │   • WebSocket signaling (/host/stream)                               │   │
│  │   • Receives Setup message with transport info                        │   │
│  │   • Dispatches session_token + webtransport_url to native            │   │
│  │   • Renders video (via WebTransport or WebRTC)                       │   │
│  │   • Plays audio                                                       │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                              │                                               │
│                              │ AndroidBridge                                 │
│                              ▼                                               │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    Native Input Client (Cronet)                       │   │
│  │                                                                       │   │
│  │   • WebTransport connection to /webtransport/input?token=XXX         │   │
│  │   • Full input processing control (gamepad-to-mouse, etc.)           │   │
│  │   • Bidirectional streams for each input channel                      │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ QUIC/HTTP3
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SERVER (:4433)                                  │
│                                                                              │
│   /webtransport        → Video datagrams + Audio stream (from WebView)      │
│   /webtransport/input  → Input bidirectional streams (from native)          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Connection Flow

### Step 1: WebSocket Signaling (WebView)

The WebView connects to the signaling endpoint and receives transport configuration:

```kotlin
// WebView loads stream page
webView.loadUrl("https://$server/stream?hostId=$hostId&appId=$appId&hybrid=true&transport=webtransport")
```

Server responds with Setup message:
```json
{
  "Setup": {
    "ice_servers": [...],
    "session_token": "abc123-def456",
    "webtransport_url": "https://192.168.1.100:4433/webtransport",
    "webtransport_input_url": "https://192.168.1.100:4433/webtransport/input",
    "cert_hash": "a1b2c3d4e5f6...",
    "available_transports": ["webtransport", "webrtc"]
  }
}
```

### Step 2: Bridge to Native (AndroidBridge)

WebView dispatches transport info to native via JavaScript bridge:

```kotlin
class AndroidBridge(private val viewModel: StreamViewModel) {
    
    @JavascriptInterface
    fun onTransportSetup(
        sessionToken: String,
        webtransportUrl: String?,
        webtransportInputUrl: String?,
        certHash: String?,
        availableTransports: String  // JSON array: ["webtransport", "webrtc"]
    ) {
        Log.d("Bridge", "Transport setup received")
        Log.d("Bridge", "  Session token: $sessionToken")
        Log.d("Bridge", "  WebTransport URL: $webtransportUrl")
        Log.d("Bridge", "  WebTransport Input URL: $webtransportInputUrl")
        Log.d("Bridge", "  Cert hash: $certHash")
        
        viewModel.onTransportSetup(
            sessionToken = sessionToken,
            webtransportUrl = webtransportUrl,
            webtransportInputUrl = webtransportInputUrl,
            certHash = certHash,
            availableTransports = parseTransports(availableTransports)
        )
    }
    
    @JavascriptInterface
    fun onTransportConnected(transportType: String) {
        // "webtransport" or "webrtc"
        Log.d("Bridge", "WebView connected via: $transportType")
        
        if (transportType == "webtransport") {
            // WebView is using WebTransport, connect native input via WebTransport
            viewModel.connectWebTransportInput()
        } else {
            // WebView is using WebRTC, connect native input via WebRTC (existing flow)
            viewModel.connectWebRtcInput()
        }
    }
    
    @JavascriptInterface
    fun onTransportFailed(transportType: String, error: String) {
        Log.w("Bridge", "Transport $transportType failed: $error")
        // Handle fallback logic
    }
}
```

### Step 3: Native WebTransport Input Connection (Cronet)

When WebView successfully connects via WebTransport, native client connects for input:

```kotlin
class WebTransportInputClient(
    private val context: Context,
    private val inputUrl: String,      // "https://server:4433/webtransport/input"
    private val sessionToken: String,
    private val certHash: String,
    private val inputHandler: InputHandler
) {
    private var cronetEngine: CronetEngine? = null
    private var bidirectionalStream: BidirectionalStream? = null
    
    // Input channel streams
    private val channelStreams = mutableMapOf<InputChannel, BidirectionalStream>()
    
    suspend fun connect(): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            // Initialize Cronet with QUIC support
            cronetEngine = CronetEngine.Builder(context)
                .enableQuic(true)
                .enableHttp2(true)
                .addQuicHint(inputUrl.toHttpUrl().host, 4433, 4433)
                .build()
            
            // Build URL with session token
            val fullUrl = "$inputUrl?token=${URLEncoder.encode(sessionToken, "UTF-8")}"
            
            // Create WebTransport-like connection using Cronet's bidirectional stream
            // Note: Cronet doesn't have native WebTransport API, but supports QUIC
            // We use bidirectional streams over HTTP/3
            
            // Connect and create input channel streams
            connectInputChannels()
            
            Result.success(Unit)
        } catch (e: Exception) {
            Log.e("WebTransport", "Connection failed", e)
            Result.failure(e)
        }
    }
    
    private suspend fun connectInputChannels() {
        // Create bidirectional streams for each input channel
        for (channel in InputChannel.values()) {
            createChannelStream(channel)
        }
    }
    
    private suspend fun createChannelStream(channel: InputChannel) {
        // Implementation depends on Cronet's bidirectional stream API
        // Each stream sends channel ID as first byte
    }
    
    // Input sending methods
    fun sendMouseMove(dx: Int, dy: Int) {
        val stream = channelStreams[InputChannel.MOUSE_RELATIVE] ?: return
        val packet = MouseRelativePacket(dx, dy).serialize()
        stream.write(packet)
    }
    
    fun sendMouseClick(button: Int, isDown: Boolean) {
        val stream = channelStreams[InputChannel.MOUSE_RELIABLE] ?: return
        val packet = MouseClickPacket(button, isDown).serialize()
        stream.write(packet)
    }
    
    fun sendKeyEvent(keyCode: Int, isDown: Boolean, modifiers: Int) {
        val stream = channelStreams[InputChannel.KEYBOARD] ?: return
        val packet = KeyboardPacket(keyCode, isDown, modifiers).serialize()
        stream.write(packet)
    }
    
    fun sendGamepadState(gamepadIndex: Int, state: GamepadState) {
        val channelId = InputChannel.CONTROLLER_0.id + gamepadIndex
        val stream = channelStreams.values.find { it.channelId == channelId } ?: return
        val packet = GamepadPacket(state).serialize()
        stream.write(packet)
    }
    
    fun disconnect() {
        channelStreams.values.forEach { it.close() }
        channelStreams.clear()
        cronetEngine?.shutdown()
    }
}

enum class InputChannel(val id: Byte) {
    MOUSE_RELIABLE(0x01),
    MOUSE_RELATIVE(0x02),
    MOUSE_ABSOLUTE(0x03),
    KEYBOARD(0x04),
    TOUCH(0x05),
    CONTROLLERS(0x06),
    CONTROLLER_0(0x10),
    // ... CONTROLLER_1 through CONTROLLER_15
    STATS(0x30)
}
```

## Cronet Setup

### Gradle Dependencies

```kotlin
// app/build.gradle.kts
dependencies {
    // Cronet for QUIC/HTTP3 support
    implementation("org.chromium.net:cronet-embedded:119.6045.31")
    
    // Or use Google Play Services version (smaller APK, requires Play Services)
    // implementation("com.google.android.gms:play-services-cronet:18.0.1")
}
```

### Cronet Initialization

```kotlin
class StreamApplication : Application() {
    
    lateinit var cronetEngine: CronetEngine
        private set
    
    override fun onCreate() {
        super.onCreate()
        
        // Initialize Cronet engine with optimal settings for streaming
        cronetEngine = CronetEngine.Builder(this)
            .enableQuic(true)
            .enableHttp2(true)
            .enableBrotli(true)
            .setStoragePath(cacheDir.absolutePath)
            .enableHttpCache(CronetEngine.Builder.HTTP_CACHE_DISK, 10 * 1024 * 1024)
            .build()
    }
}
```

### Certificate Handling for Self-Signed Certs

For local network streaming with self-signed certificates:

```kotlin
class WebTransportCertificateHandler {
    
    /**
     * Verify server certificate against expected hash.
     * The hash is SHA-256 of the DER-encoded certificate.
     */
    fun verifyCertificate(
        certificate: X509Certificate,
        expectedHash: String
    ): Boolean {
        val derEncoded = certificate.encoded
        val actualHash = MessageDigest.getInstance("SHA-256")
            .digest(derEncoded)
            .toHexString()
        
        return actualHash.equals(expectedHash, ignoreCase = true)
    }
    
    /**
     * Create a TrustManager that accepts certificates matching the expected hash.
     */
    fun createTrustManager(expectedHash: String): X509TrustManager {
        return object : X509TrustManager {
            override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
                throw CertificateException("Client certificates not supported")
            }
            
            override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
                if (chain.isEmpty()) {
                    throw CertificateException("Empty certificate chain")
                }
                
                if (!verifyCertificate(chain[0], expectedHash)) {
                    throw CertificateException("Certificate hash mismatch")
                }
            }
            
            override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
        }
    }
}

private fun ByteArray.toHexString(): String = 
    joinToString("") { "%02x".format(it) }
```

## Fallback to WebRTC

If WebTransport fails, the system automatically falls back to WebRTC:

```kotlin
class StreamViewModel : ViewModel() {
    
    private var transportType: TransportType = TransportType.UNKNOWN
    private var webTransportClient: WebTransportInputClient? = null
    private var webRtcClient: WebRtcInputClient? = null  // Your existing implementation
    
    fun onTransportSetup(
        sessionToken: String,
        webtransportUrl: String?,
        webtransportInputUrl: String?,
        certHash: String?,
        availableTransports: List<String>
    ) {
        this.sessionToken = sessionToken
        this.webtransportInputUrl = webtransportInputUrl
        this.certHash = certHash
        this.availableTransports = availableTransports
        
        // Wait for WebView to report which transport it connected with
    }
    
    fun connectWebTransportInput() {
        viewModelScope.launch {
            val url = webtransportInputUrl ?: run {
                Log.e("Stream", "No WebTransport input URL")
                fallbackToWebRtc("missing_url")
                return@launch
            }
            val hash = certHash ?: run {
                Log.e("Stream", "No certificate hash")
                fallbackToWebRtc("missing_cert_hash")
                return@launch
            }
            
            webTransportClient = WebTransportInputClient(
                context = getApplication(),
                inputUrl = url,
                sessionToken = sessionToken!!,
                certHash = hash,
                inputHandler = inputHandler
            )
            
            webTransportClient!!.connect()
                .onSuccess {
                    transportType = TransportType.WEBTRANSPORT
                    Log.i("Stream", "WebTransport input connected")
                }
                .onFailure { error ->
                    Log.w("Stream", "WebTransport input failed", error)
                    fallbackToWebRtc(error.message ?: "unknown")
                }
        }
    }
    
    fun connectWebRtcInput() {
        // Your existing WebRTC input connection logic
        viewModelScope.launch {
            webRtcClient = WebRtcInputClient(/* ... */)
            webRtcClient!!.connect(sessionToken!!)
            transportType = TransportType.WEBRTC
        }
    }
    
    private fun fallbackToWebRtc(reason: String) {
        Log.w("Stream", "Falling back to WebRTC: $reason")
        
        // Notify WebView to switch transport
        webView?.evaluateJavascript(
            "window.dispatchEvent(new CustomEvent('fallbackToWebRtc', { detail: { reason: '$reason' } }))",
            null
        )
        
        // Connect via WebRTC
        connectWebRtcInput()
    }
    
    // Input methods - delegate to active client
    fun sendMouseMove(dx: Int, dy: Int) {
        when (transportType) {
            TransportType.WEBTRANSPORT -> webTransportClient?.sendMouseMove(dx, dy)
            TransportType.WEBRTC -> webRtcClient?.sendMouseMove(dx, dy)
            else -> Log.w("Stream", "No transport connected")
        }
    }
    
    // ... other input methods
}

enum class TransportType {
    UNKNOWN,
    WEBTRANSPORT,
    WEBRTC
}
```

## WebView JavaScript Updates

Update the WebView's JavaScript to dispatch transport info to native:

```typescript
// In stream/index.ts - add to onMessage handler for Setup
else if ("Setup" in message) {
    const setup = message.Setup;
    
    // Dispatch to Android bridge
    if (window.AndroidBridge?.onTransportSetup) {
        window.AndroidBridge.onTransportSetup(
            setup.session_token ?? "",
            setup.webtransport_url ?? null,
            setup.webtransport_input_url ?? null,
            setup.cert_hash ?? null,
            JSON.stringify(setup.available_transports ?? [])
        );
    }
    
    // Continue with transport selection...
}

// After transport connects successfully
private onTransportConnected(type: "webtransport" | "webrtc") {
    if (window.AndroidBridge?.onTransportConnected) {
        window.AndroidBridge.onTransportConnected(type);
    }
}

// On transport failure
private onTransportFailed(type: "webtransport" | "webrtc", error: string) {
    if (window.AndroidBridge?.onTransportFailed) {
        window.AndroidBridge.onTransportFailed(type, error);
    }
}
```

## URL Parameters

The stream page accepts these URL parameters:

| Parameter | Values | Description |
|-----------|--------|-------------|
| `hostId` | number | Host ID to stream from |
| `appId` | number | Application ID to launch |
| `hybrid` | `true`/`false` | Enable hybrid mode (separate input connection) |
| `transport` | `auto`/`webtransport`/`webrtc` | Preferred transport |

Example:
```
https://server/stream?hostId=1&appId=2&hybrid=true&transport=webtransport
```

## Input Protocol

### Channel IDs

| ID | Channel | Description |
|----|---------|-------------|
| `0x01` | MOUSE_RELIABLE | Mouse clicks, wheel events (ordered) |
| `0x02` | MOUSE_RELATIVE | Mouse movement deltas (unordered) |
| `0x03` | MOUSE_ABSOLUTE | Absolute mouse position (touch-to-mouse) |
| `0x04` | KEYBOARD | Keyboard events (ordered) |
| `0x05` | TOUCH | Touch events (ordered) |
| `0x06` | CONTROLLERS | Combined gamepad data (unordered) |
| `0x10-0x1F` | CONTROLLER_0-15 | Individual gamepad channels |
| `0x30` | STATS | Latency measurement channel |

### Stream Initialization

When creating a bidirectional stream for input:
1. Client opens bidirectional stream
2. Client sends channel ID as first byte: `[channel_id: u8]`
3. Server acknowledges by accepting the stream
4. Bidirectional data flow begins

### Packet Format

```
┌─────────────────────────────────────────────┐
│  Length (2 bytes, big-endian)               │
│  Payload (channel-specific data)            │
└─────────────────────────────────────────────┘
```

## Testing Checklist

- [ ] WebView connects via WebTransport when available
- [ ] `onTransportSetup` receives correct URLs and cert hash
- [ ] `onTransportConnected("webtransport")` triggers native WebTransport input
- [ ] Native WebTransport input connects to `/webtransport/input`
- [ ] Session token validation works
- [ ] Input channels created successfully
- [ ] Mouse/keyboard/gamepad input works
- [ ] Fallback to WebRTC works when WebTransport fails
- [ ] Stats display shows correct transport type
- [ ] Connection survives network changes (WiFi ↔ Mobile)

## Troubleshooting

### WebTransport Connection Fails

1. **Check port accessibility**: Ensure UDP port 4433 is not blocked
2. **Check certificate**: Verify cert hash matches server's certificate
3. **Check Cronet version**: Ensure QUIC is enabled and supported
4. **Check network**: Some corporate networks block QUIC

### Session Token Invalid

1. **Timing**: Ensure WebView connects before native input
2. **Token format**: Token should be passed as URL query parameter
3. **Token expiry**: Tokens expire after 30 seconds if not claimed

### High Input Latency

1. **Check transport type**: Ensure WebTransport is being used (not WebRTC relay)
2. **Network path**: WebTransport should have direct connection
3. **Channel ordering**: Use unordered channels for mouse movement

## Migration from WebRTC-Only

If you have an existing WebRTC implementation:

1. **Keep existing code**: WebRTC remains as fallback
2. **Add Cronet dependency**: For WebTransport support
3. **Update AndroidBridge**: Add new callbacks for transport setup
4. **Add WebTransportInputClient**: New class for WebTransport input
5. **Update ViewModel**: Handle both transport types
6. **Test both paths**: Verify WebTransport works and fallback to WebRTC works
