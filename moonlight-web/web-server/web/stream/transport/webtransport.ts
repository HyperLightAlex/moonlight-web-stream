import { StreamSignalingMessage, TransportChannelId } from "../../api_bindings.js";
import { Logger } from "../log.js";
import { DataTransportChannel, Transport, TRANSPORT_CHANNEL_OPTIONS, TransportAudioSetup, TransportChannel, TransportChannelIdKey, TransportChannelIdValue, TransportVideoSetup, AudioTrackTransportChannel, VideoTrackTransportChannel, TrackTransportChannel } from "./index.js";

// Stats tracking for WebTransport
interface WebTransportStats {
    // Connection timing
    connectionStartTime: number | null
    
    // Datagram stats (video)
    datagramsReceived: number
    datagramsLost: number
    datagramBytes: number
    
    // Stream stats (audio/input)
    streamBytesReceived: number
    streamBytesSent: number
    
    // Frame tracking
    framesReceived: number
    framesDecoded: number
    framesDropped: number
    lastFrameTime: number | null
    frameTimestamps: number[] // Rolling window for FPS calculation
    
    // Latency tracking
    rttMeasurements: number[] // Rolling window for RTT
    lastRttMs: number | null
    
    // Jitter tracking (variation in packet arrival time)
    lastPacketArrivalTime: number | null
    jitterMs: number
}

export class WebTransportTransport implements Transport {
    implementationName: string = "webtransport"

    private logger: Logger | null

    private transport: WebTransport | null = null
    private inputTransport: WebTransport | null = null // Separate connection for input (hybrid mode)
    private sessionToken: string | null = null
    private certHash: string | null = null // SHA-256 hash of server certificate for pinning

    // Channels
    private channels: Map<TransportChannelIdValue, TransportChannel> = new Map()

    // Video datagram reader
    private videoDatagramReader: ReadableStreamDefaultReader<Uint8Array> | null = null
    private videoDatagramWriter: WritableStreamDefaultWriter<Uint8Array> | null = null

    // Audio stream reader
    private audioStreamReader: ReadableStreamDefaultReader<Uint8Array> | null = null

    // Input channels (bidirectional streams)
    private inputStreams: Map<TransportChannelIdValue, {
        sendStream: WritableStreamDefaultWriter<Uint8Array>,
        recvStream: ReadableStreamDefaultReader<Uint8Array>
    }> = new Map()

    // Stats tracking
    private stats: WebTransportStats = {
        connectionStartTime: null,
        datagramsReceived: 0,
        datagramsLost: 0,
        datagramBytes: 0,
        streamBytesReceived: 0,
        streamBytesSent: 0,
        framesReceived: 0,
        framesDecoded: 0,
        framesDropped: 0,
        lastFrameTime: null,
        frameTimestamps: [],
        rttMeasurements: [],
        lastRttMs: null,
        lastPacketArrivalTime: null,
        jitterMs: 0,
    }

    // RTT measurement interval
    private rttMeasurementInterval: number | null = null
    private lastSequenceNumber: number = -1

    constructor(logger?: Logger) {
        this.logger = logger ?? null
    }

    async initTransport(url: string, certHash: string, sessionToken?: string) {
        this.logger?.debug(`[WebTransport]: Creating transport connection to ${url}`)
        this.certHash = certHash
        this.sessionToken = sessionToken ?? null

        try {
            // Create main transport connection
            // For local development with self-signed certs, we need serverCertificateHashes
            const transportOptions: WebTransportOptions = {
                serverCertificateHashes: [{
                    algorithm: "sha-256",
                    value: this.hexToUint8Array(certHash)
                }]
            }

            this.transport = new WebTransport(url, transportOptions)
            
            // Wait for connection to be ready
            await this.transport.ready
            this.stats.connectionStartTime = performance.now()
            this.logger?.debug(`[WebTransport]: Main transport connection ready`)

            // Set up video datagrams (unreliable, for video frames)
            const datagrams = this.transport.datagrams
            this.videoDatagramReader = datagrams.readable.getReader()
            this.videoDatagramWriter = datagrams.writable.getWriter()

            // Set up audio stream (unidirectional, server -> client)
            // The server will create this stream, so we need to accept it
            // For now, we'll set it up when audio is configured
            this.logger?.debug(`[WebTransport]: Audio stream will be set up when audio is configured`)

            // Set up input channels (bidirectional streams)
            // These will be created by the client when needed
            this.setupInputChannels()

            // Start RTT measurement (ping/pong via a dedicated stream)
            this.startRttMeasurement()

            // Handle connection state changes
            this.transport.closed.then(() => {
                this.logger?.debug(`[WebTransport]: Main transport connection closed`)
                this.stopRttMeasurement()
                if (this.onclose) {
                    this.onclose("disconnect")
                }
            }).catch((err) => {
                this.logger?.debug(`[WebTransport]: Main transport connection error: ${err}`)
                this.stopRttMeasurement()
                if (this.onclose) {
                    this.onclose("failed")
                }
            })

            // In hybrid mode, input is handled by the native client via WebRTC (not WebTransport)
            // The session token is dispatched to the native client which connects separately
            // This provides lowest latency and allows native input processing (gamepad-to-mouse, etc.)
            // We do NOT create a WebTransport input connection here - native WebRTC handles it
            if (this.sessionToken) {
                this.logger?.debug(`[WebTransport]: Hybrid mode - input will be handled by native WebRTC client`)
            }

            if (this.onconnected) {
                this.onconnected()
            }
        } catch (err) {
            this.logger?.debug(`[WebTransport]: Failed to create transport: ${err}`)
            console.error(`[WebTransport]: Failed to create transport:`, err)
            if (this.onclose) {
                this.onclose("failednoconnect")
            }
            throw err
        }
    }

    private async initInputTransport(baseUrl: string, certHash: string, sessionToken: string) {
        this.logger?.debug(`[WebTransport]: Creating input transport connection (hybrid mode)`)

        try {
            // Create input transport connection using /input path
            // The server differentiates main vs input sessions based on path:
            // - /webtransport (or /wt) -> main session (video/audio)
            // - /webtransport/input (or /wt/input) -> input session
            // Session token is passed as query parameter for validation
            const inputUrl = `${baseUrl}/input?token=${encodeURIComponent(sessionToken)}`
            
            const transportOptions: WebTransportOptions = {
                serverCertificateHashes: [{
                    algorithm: "sha-256",
                    value: this.hexToUint8Array(certHash)
                }]
            }

            this.logger?.debug(`[WebTransport]: Input URL: ${inputUrl}`)
            this.inputTransport = new WebTransport(inputUrl, transportOptions)
            
            await this.inputTransport.ready
            this.logger?.debug(`[WebTransport]: Input transport connection ready`)

            // Set up input channels on the input transport
            this.setupInputChannelsOnInputTransport()

            this.inputTransport.closed.then(() => {
                this.logger?.debug(`[WebTransport]: Input transport connection closed`)
            }).catch((err) => {
                this.logger?.debug(`[WebTransport]: Input transport connection error: ${err}`)
            })
        } catch (err) {
            this.logger?.debug(`[WebTransport]: Failed to create input transport: ${err}`)
            console.error(`[WebTransport]: Failed to create input transport:`, err)
            // Input transport failure is not fatal, but log it
        }
    }

    private setupInputChannels() {
        // Create bidirectional streams for each input channel type
        // These will be created on-demand when the channel is first used
        // For now, we just initialize the channel map
    }

    private setupInputChannelsOnInputTransport() {
        // Set up input channels on the separate input transport
        // Similar to setupInputChannels but uses inputTransport
    }

    private startRttMeasurement() {
        // Measure RTT every second using timestamp-based measurement
        // Since WebTransport doesn't have built-in ping/pong, we estimate RTT
        // from the connection's inherent QUIC RTT when available
        this.rttMeasurementInterval = window.setInterval(() => {
            this.measureRtt()
        }, 1000)
    }

    private stopRttMeasurement() {
        if (this.rttMeasurementInterval !== null) {
            clearInterval(this.rttMeasurementInterval)
            this.rttMeasurementInterval = null
        }
    }

    private async measureRtt() {
        // WebTransport over QUIC has RTT measurements built into the protocol
        // However, the browser API doesn't expose this directly
        // We can estimate RTT by measuring round-trip time for a small message
        // For now, we'll use connection age as a proxy and rely on server-reported stats
        
        // Update connection uptime
        if (this.stats.connectionStartTime) {
            const uptime = performance.now() - this.stats.connectionStartTime
            // This is just for tracking, actual RTT comes from protocol-level measurements
        }
    }

    private hexToUint8Array(hex: string): Uint8Array {
        const bytes = new Uint8Array(hex.length / 2)
        for (let i = 0; i < hex.length; i += 2) {
            bytes[i / 2] = parseInt(hex.substr(i, 2), 16)
        }
        return bytes
    }

    // Stats tracking methods
    recordDatagramReceived(bytes: number, sequenceNumber?: number) {
        const now = performance.now()
        
        this.stats.datagramsReceived++
        this.stats.datagramBytes += bytes
        
        // Track packet loss by checking sequence gaps
        if (sequenceNumber !== undefined && this.lastSequenceNumber >= 0) {
            const expected = (this.lastSequenceNumber + 1) & 0xFFFF // 16-bit sequence
            if (sequenceNumber !== expected) {
                // Calculate lost packets (handle wraparound)
                let lost = sequenceNumber - expected
                if (lost < 0) lost += 0x10000
                if (lost > 0 && lost < 1000) { // Sanity check
                    this.stats.datagramsLost += lost
                }
            }
        }
        if (sequenceNumber !== undefined) {
            this.lastSequenceNumber = sequenceNumber
        }
        
        // Track jitter (variation in inter-packet arrival time)
        if (this.stats.lastPacketArrivalTime !== null) {
            const interArrival = now - this.stats.lastPacketArrivalTime
            // Simple exponential moving average for jitter
            const instantJitter = Math.abs(interArrival - 16.67) // Assuming ~60fps target
            this.stats.jitterMs = this.stats.jitterMs * 0.9 + instantJitter * 0.1
        }
        this.stats.lastPacketArrivalTime = now
    }

    recordFrameReceived() {
        const now = performance.now()
        this.stats.framesReceived++
        
        // Keep a rolling window of frame timestamps for FPS calculation (last 2 seconds)
        this.stats.frameTimestamps.push(now)
        const twoSecondsAgo = now - 2000
        this.stats.frameTimestamps = this.stats.frameTimestamps.filter(t => t > twoSecondsAgo)
    }

    recordFrameDecoded() {
        this.stats.framesDecoded++
    }

    recordFrameDropped() {
        this.stats.framesDropped++
    }

    recordStreamBytesReceived(bytes: number) {
        this.stats.streamBytesReceived += bytes
    }

    recordStreamBytesSent(bytes: number) {
        this.stats.streamBytesSent += bytes
    }

    recordRtt(rttMs: number) {
        this.stats.lastRttMs = rttMs
        this.stats.rttMeasurements.push(rttMs)
        // Keep last 10 measurements for averaging
        if (this.stats.rttMeasurements.length > 10) {
            this.stats.rttMeasurements.shift()
        }
    }

    // Calculate current FPS from frame timestamps
    private calculateFps(): number {
        if (this.stats.frameTimestamps.length < 2) {
            return 0
        }
        
        const oldest = this.stats.frameTimestamps[0]
        const newest = this.stats.frameTimestamps[this.stats.frameTimestamps.length - 1]
        const duration = (newest - oldest) / 1000 // seconds
        
        if (duration <= 0) return 0
        return (this.stats.frameTimestamps.length - 1) / duration
    }

    onsendmessage: ((message: StreamSignalingMessage) => void) | null = null
    async onReceiveMessage(message: StreamSignalingMessage) {
        // WebTransport doesn't use signaling messages like WebRTC
        // This is kept for interface compatibility but shouldn't be called
        this.logger?.debug(`[WebTransport]: Received signaling message (not used): ${JSON.stringify(message)}`)
    }

    onconnected: (() => void) | null = null
    ondisconnected: (() => void) | null = null
    onclose: ((shutdown: "failednoconnect" | "failed" | "disconnect") => void) | null = null

    getChannel(id: TransportChannelIdValue): TransportChannel {
        if (this.channels.has(id)) {
            return this.channels.get(id)!
        }

        // Get channel options - default to reliable/ordered for unknown channels
        let options = { ordered: true, reliable: true }
        for (const key in TransportChannelId) {
            if ((TransportChannelId as Record<string, number>)[key] === id) {
                const channelKey = key as TransportChannelIdKey
                options = TRANSPORT_CHANNEL_OPTIONS[channelKey]
                break
            }
        }

        let channel: TransportChannel

        if (id === TransportChannelId.HOST_VIDEO) {
            // Video uses datagrams (unreliable) - pass this for stats tracking
            channel = new WebTransportVideoDatagramChannel(this.videoDatagramReader, this, this.logger)
        } else if (id === TransportChannelId.HOST_AUDIO) {
            // Audio uses unidirectional stream (reliable)
            channel = new WebTransportAudioStreamChannel(this.audioStreamReader, this, this.logger)
        } else {
            // Input channels use bidirectional streams (reliable)
            channel = new WebTransportDataChannel(id, options, this, this.logger)
        }

        this.channels.set(id, channel)
        return channel
    }

    async setupHostVideo(setup: TransportVideoSetup) {
        this.logger?.debug(`[WebTransport]: Setting up host video: ${JSON.stringify(setup)}`)
        // Video is already set up via datagrams in initTransport
        // The video channel will read from the datagram reader
    }

    async setupHostAudio(setup: TransportAudioSetup) {
        this.logger?.debug(`[WebTransport]: Setting up host audio: ${JSON.stringify(setup)}`)
        
        // Accept the unidirectional audio stream from the server
        if (!this.transport) {
            throw new Error("Transport not initialized")
        }

        // The server creates the audio stream as a unidirectional stream
        // We accept it via incomingUnidirectionalStreams (WebTransport API)
        const incomingStreams = this.transport.incomingUnidirectionalStreams
        const reader = incomingStreams.getReader()
        
        // Read the first stream (should be audio)
        const { value: stream, done } = await reader.read()
        if (done || !stream) {
            throw new Error("Failed to receive audio stream from server")
        }

        this.audioStreamReader = stream.getReader()
        this.logger?.debug(`[WebTransport]: Audio stream accepted`)
    }

    async close() {
        this.logger?.debug(`[WebTransport]: Closing transport`)
        
        this.stopRttMeasurement()

        // Close video datagram reader/writer
        if (this.videoDatagramReader) {
            await this.videoDatagramReader.cancel()
            this.videoDatagramReader = null
        }
        if (this.videoDatagramWriter) {
            await this.videoDatagramWriter.close()
            this.videoDatagramWriter = null
        }

        // Close audio stream reader
        if (this.audioStreamReader) {
            await this.audioStreamReader.cancel()
            this.audioStreamReader = null
        }

        // Close input streams
        for (const [id, stream] of this.inputStreams) {
            await stream.sendStream.close()
            await stream.recvStream.cancel()
        }
        this.inputStreams.clear()

        // Close transports
        if (this.transport) {
            this.transport.close()
            this.transport = null
        }
        if (this.inputTransport) {
            this.inputTransport.close()
            this.inputTransport = null
        }

        if (this.ondisconnected) {
            this.ondisconnected()
        }
    }

    async getStats(): Promise<Record<string, string>> {
        const statsData: Record<string, string> = {}
        
        // Transport identifier
        statsData.transportType = "webtransport"
        
        // Connection state
        if (this.transport) {
            statsData.wtState = "connected"
        } else {
            statsData.wtState = "disconnected"
        }
        
        // Calculate FPS
        const fps = this.calculateFps()
        if (fps > 0) {
            statsData.wtFps = fps.toFixed(1)
        }
        
        // Packet stats
        statsData.wtDatagramsReceived = this.stats.datagramsReceived.toString()
        statsData.wtDatagramsLost = this.stats.datagramsLost.toString()
        
        // Calculate packet loss percentage
        const totalPackets = this.stats.datagramsReceived + this.stats.datagramsLost
        if (totalPackets > 0) {
            const lossPercent = (this.stats.datagramsLost / totalPackets) * 100
            statsData.wtPacketLossPercent = lossPercent.toFixed(2)
        }
        
        // Bytes transferred
        statsData.wtBytesReceived = this.stats.datagramBytes.toString()
        statsData.wtStreamBytesReceived = this.stats.streamBytesReceived.toString()
        statsData.wtStreamBytesSent = this.stats.streamBytesSent.toString()
        
        // Frame stats
        statsData.wtFramesReceived = this.stats.framesReceived.toString()
        statsData.wtFramesDecoded = this.stats.framesDecoded.toString()
        statsData.wtFramesDropped = this.stats.framesDropped.toString()
        
        // RTT (if we have measurements from server-side stats)
        if (this.stats.lastRttMs !== null) {
            statsData.wtRttMs = this.stats.lastRttMs.toFixed(1)
        }
        
        // Average RTT
        if (this.stats.rttMeasurements.length > 0) {
            const avgRtt = this.stats.rttMeasurements.reduce((a, b) => a + b, 0) / this.stats.rttMeasurements.length
            statsData.wtAvgRttMs = avgRtt.toFixed(1)
        }
        
        // Jitter
        if (this.stats.jitterMs > 0) {
            statsData.wtJitterMs = this.stats.jitterMs.toFixed(1)
        }
        
        // Connection uptime
        if (this.stats.connectionStartTime) {
            const uptimeMs = performance.now() - this.stats.connectionStartTime
            statsData.wtUptimeSec = (uptimeMs / 1000).toFixed(0)
        }
        
        // Bitrate calculation (bits per second over last second)
        if (this.stats.connectionStartTime) {
            const durationSec = (performance.now() - this.stats.connectionStartTime) / 1000
            if (durationSec > 0) {
                const bitsReceived = this.stats.datagramBytes * 8
                const bitrateKbps = (bitsReceived / durationSec) / 1000
                statsData.wtBitrateKbps = bitrateKbps.toFixed(0)
            }
        }
        
        return statsData
    }

    async getConnectionInfo(): Promise<{ connectionType: string, isRelay: boolean, rttMs: number }> {
        // WebTransport uses QUIC which is typically direct (no relay like TURN)
        const avgRtt = this.stats.rttMeasurements.length > 0
            ? this.stats.rttMeasurements.reduce((a, b) => a + b, 0) / this.stats.rttMeasurements.length
            : this.stats.lastRttMs ?? -1
            
        return {
            connectionType: "webtransport-quic",
            isRelay: false, // WebTransport/QUIC doesn't use relay servers
            rttMs: avgRtt
        }
    }

    // Internal method to get or create a bidirectional stream for an input channel
    async getOrCreateInputStream(channelId: TransportChannelIdValue): Promise<{
        sendStream: WritableStreamDefaultWriter<Uint8Array>,
        recvStream: ReadableStreamDefaultReader<Uint8Array>
    }> {
        if (this.inputStreams.has(channelId)) {
            return this.inputStreams.get(channelId)!
        }

        // Use input transport if available (hybrid mode), otherwise use main transport
        const transport = this.inputTransport || this.transport
        if (!transport) {
            throw new Error("Transport not initialized")
        }

        this.logger?.debug(`[WebTransport]: Creating bidirectional stream for channel ${channelId}`)

        // Create bidirectional stream
        const stream = await transport.createBidirectionalStream()
        const sendStream = stream.writable.getWriter()
        const recvStream = stream.readable.getReader()

        // Send channel ID as the first byte so server knows which channel this is
        const channelIdByte = new Uint8Array([channelId])
        await sendStream.write(channelIdByte)
        this.logger?.debug(`[WebTransport]: Sent channel ID ${channelId} on new stream`)

        const streamInfo = { sendStream, recvStream }
        this.inputStreams.set(channelId, streamInfo)
        return streamInfo
    }
}

// Video channel using datagrams (unreliable)
class WebTransportVideoDatagramChannel implements VideoTrackTransportChannel {
    readonly type: "videotrack" = "videotrack"
    readonly canReceive: boolean = true
    readonly canSend: boolean = false

    private datagramReader: ReadableStreamDefaultReader<Uint8Array> | null
    private trackListeners: Set<(track: MediaStreamTrack) => void> = new Set()
    private logger: Logger | null
    private transport: WebTransportTransport

    constructor(datagramReader: ReadableStreamDefaultReader<Uint8Array> | null, transport: WebTransportTransport, logger?: Logger | null) {
        this.datagramReader = datagramReader
        this.transport = transport
        this.logger = logger ?? null
    }

    setTrack(track: MediaStreamTrack | null): void {
        // WebTransport video uses datagrams, not MediaStreamTrack
        // This is kept for interface compatibility
        this.logger?.debug(`[WebTransport]: setTrack called on video datagram channel (not used)`)
    }

    addTrackListener(listener: (track: MediaStreamTrack) => void): void {
        // WebTransport video uses datagrams, not MediaStreamTrack
        // This is kept for interface compatibility
        this.trackListeners.add(listener)
    }

    removeTrackListener(listener: (track: MediaStreamTrack) => void): void {
        this.trackListeners.delete(listener)
    }

    // Custom method for reading video datagrams
    async readDatagram(): Promise<Uint8Array | null> {
        if (!this.datagramReader) {
            return null
        }

        try {
            const { value, done } = await this.datagramReader.read()
            if (done) {
                return null
            }
            
            // Record stats - parse header to get sequence number
            // Packet format: [timestamp: u32][sequence: u16][is_last: u8][payload]
            if (value.length >= 7) {
                const sequenceNumber = (value[4] << 8) | value[5]
                this.transport.recordDatagramReceived(value.length, sequenceNumber)
                
                // Record frame if this is the last packet of a frame
                if (value[6] === 1) {
                    this.transport.recordFrameReceived()
                }
            } else {
                this.transport.recordDatagramReceived(value.length)
            }
            
            return value
        } catch (err) {
            this.logger?.debug(`[WebTransport]: Error reading video datagram: ${err}`)
            return null
        }
    }
}

// Audio channel using unidirectional stream (reliable)
class WebTransportAudioStreamChannel implements AudioTrackTransportChannel {
    readonly type: "audiotrack" = "audiotrack"
    readonly canReceive: boolean = true
    readonly canSend: boolean = false

    private streamReader: ReadableStreamDefaultReader<Uint8Array> | null
    private trackListeners: Set<(track: MediaStreamTrack) => void> = new Set()
    private logger: Logger | null
    private transport: WebTransportTransport

    constructor(streamReader: ReadableStreamDefaultReader<Uint8Array> | null, transport: WebTransportTransport, logger?: Logger | null) {
        this.streamReader = streamReader
        this.transport = transport
        this.logger = logger ?? null
    }

    setTrack(track: MediaStreamTrack | null): void {
        // WebTransport audio uses streams, not MediaStreamTrack
        // This is kept for interface compatibility
        this.logger?.debug(`[WebTransport]: setTrack called on audio stream channel (not used)`)
    }

    addTrackListener(listener: (track: MediaStreamTrack) => void): void {
        // WebTransport audio uses streams, not MediaStreamTrack
        // This is kept for interface compatibility
        this.trackListeners.add(listener)
    }

    removeTrackListener(listener: (track: MediaStreamTrack) => void): void {
        this.trackListeners.delete(listener)
    }

    // Custom method for reading audio stream data
    async readAudioData(): Promise<Uint8Array | null> {
        if (!this.streamReader) {
            return null
        }

        try {
            const { value, done } = await this.streamReader.read()
            if (done) {
                return null
            }
            
            // Record stats
            this.transport.recordStreamBytesReceived(value.length)
            
            return value
        } catch (err) {
            this.logger?.debug(`[WebTransport]: Error reading audio stream: ${err}`)
            return null
        }
    }
}

// Data channel using bidirectional streams (reliable)
class WebTransportDataChannel implements DataTransportChannel {
    readonly type: "data" = "data"
    readonly canReceive: boolean = true
    readonly canSend: boolean = true

    private channelId: TransportChannelIdValue
    private options: { ordered: boolean, reliable: boolean }
    private transport: WebTransportTransport
    private logger: Logger | null

    private receiveListeners: Set<(data: ArrayBuffer) => void> = new Set()
    private sendStream: WritableStreamDefaultWriter<Uint8Array> | null = null
    private recvStream: ReadableStreamDefaultReader<Uint8Array> | null = null
    private recvLoopRunning: boolean = false

    constructor(
        channelId: TransportChannelIdValue,
        options: { ordered: boolean, reliable: boolean },
        transport: WebTransportTransport,
        logger?: Logger | null
    ) {
        this.channelId = channelId
        this.options = options
        this.transport = transport
        this.logger = logger ?? null
    }

    addReceiveListener(listener: (data: ArrayBuffer) => void): void {
        this.receiveListeners.add(listener)
        this.startReceiveLoop()
    }

    removeReceiveListener(listener: (data: ArrayBuffer) => void): void {
        this.receiveListeners.delete(listener)
    }

    send(message: ArrayBuffer): void {
        this.sendInternal(new Uint8Array(message))
    }

    private async sendInternal(data: Uint8Array): Promise<void> {
        if (!this.sendStream) {
            // Get or create the bidirectional stream
            const streamInfo = await this.transport.getOrCreateInputStream(this.channelId)
            this.sendStream = streamInfo.sendStream
            this.recvStream = streamInfo.recvStream
            this.startReceiveLoop()
        }

        try {
            await this.sendStream.write(data)
            this.transport.recordStreamBytesSent(data.length)
        } catch (err) {
            this.logger?.debug(`[WebTransport]: Error sending data on channel ${this.channelId}: ${err}`)
        }
    }

    private async startReceiveLoop(): Promise<void> {
        if (this.recvLoopRunning || !this.recvStream) {
            return
        }

        this.recvLoopRunning = true

        // Start reading loop
        while (this.recvStream && this.receiveListeners.size > 0) {
            try {
                const { value, done } = await this.recvStream.read()
                if (done) {
                    break
                }

                // Record stats
                this.transport.recordStreamBytesReceived(value.length)

                // Notify all listeners
                const data = value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength)
                for (const listener of this.receiveListeners) {
                    listener(data)
                }
            } catch (err) {
                this.logger?.debug(`[WebTransport]: Error receiving data on channel ${this.channelId}: ${err}`)
                break
            }
        }

        this.recvLoopRunning = false
    }

    estimatedBufferedBytes(): number | null {
        // WebTransport streams don't expose buffered bytes directly
        // Return null to indicate unknown
        return null
    }
}
