import { PrimaryNegotiationRole, StreamSignalingMessage, TransportChannelId } from "../../api_bindings.js";
import { Logger } from "../log.js";
import { DataTransportChannel, Transport, TRANSPORT_CHANNEL_OPTIONS, TransportAudioSetup, TransportChannel, TransportChannelIdKey, TransportChannelIdValue, TransportVideoSetup, AudioTrackTransportChannel, VideoTrackTransportChannel, TrackTransportChannel } from "./index.js";

export class WebRTCTransport implements Transport {
    implementationName: string = "webrtc"

    private logger: Logger | null
    private negotiationRole: PrimaryNegotiationRole

    private peer: RTCPeerConnection | null = null
    private previousVideoStatsSample: {
        jitterBufferDelay: number
        jitterBufferTargetDelay: number
        jitterBufferMinimumDelay: number
        jitterBufferEmittedCount: number
        totalDecodeTime: number
        totalProcessingDelay: number
        framesDecoded: number
    } | null = null

    constructor(logger?: Logger, negotiationRole: PrimaryNegotiationRole = "clientoffer") {
        this.logger = logger ?? null
        this.negotiationRole = negotiationRole
    }

    async initPeer(configuration?: RTCConfiguration) {
        this.logger?.debug(`Creating Client Peer`)

        if (this.peer) {
            this.logger?.debug(`Cannot create Peer because a Peer already exists`)
            return
        }

        // Configure web rtc
        this.peer = new RTCPeerConnection(configuration)
        this.peer.addEventListener("error", this.onError.bind(this))

        this.peer.addEventListener("negotiationneeded", this.onNegotiationNeeded.bind(this))
        this.peer.addEventListener("icecandidate", this.onIceCandidate.bind(this))
        this.peer.addEventListener("datachannel", this.onDataChannel.bind(this))

        this.peer.addEventListener("connectionstatechange", this.onConnectionStateChange.bind(this))
        this.peer.addEventListener("iceconnectionstatechange", this.onIceConnectionStateChange.bind(this))
        this.peer.addEventListener("icegatheringstatechange", this.onIceGatheringStateChange.bind(this))

        this.peer.addEventListener("track", this.onTrack.bind(this))

        this.initChannels()

        // Maybe we already received data
        if (this.remoteDescription) {
            await this.handleRemoteDescription(this.remoteDescription)
        } else if (this.negotiationRole === "clientoffer") {
            await this.onNegotiationNeeded()
        } else {
            this.logger?.debug("Waiting for server-created offer before starting primary WebRTC negotiation")
        }
        await this.tryDequeueIceCandidates()
    }

    private onError(event: Event) {
        this.logger?.debug(`Web Socket or WebRtcPeer Error`)

        console.error(`Web Socket or WebRtcPeer Error`, event)
    }

    onsendmessage: ((message: StreamSignalingMessage) => void) | null = null
    private sendMessage(message: StreamSignalingMessage) {
        if (this.onsendmessage) {
            this.onsendmessage(message)
        } else {
            this.logger?.debug("Failed to call onicecandidate because no handler is set")
        }
    }
    async onReceiveMessage(message: StreamSignalingMessage) {
        if ("Description" in message) {
            const description = message.Description;
            console.info(`[WebRTC]: Received remote description type: ${description.ty}`)
            await this.handleRemoteDescription({
                type: description.ty as RTCSdpType,
                sdp: description.sdp
            })
        } else if ("AddIceCandidate" in message) {
            const candidate = message.AddIceCandidate
            console.info(`[WebRTC]: Received ICE candidate: ${candidate.candidate?.substring(0, 50)}...`)
            await this.addIceCandidate({
                candidate: candidate.candidate,
                sdpMid: candidate.sdp_mid,
                sdpMLineIndex: candidate.sdp_mline_index,
                usernameFragment: candidate.username_fragment
            })
        }
    }

    private async onNegotiationNeeded() {
        // We're polite
        if (!this.peer) {
            this.logger?.debug("OnNegotiationNeeded without a peer")
            return
        }

        console.info(`[WebRTC]: Negotiation needed, creating local description...`)
        await this.peer.setLocalDescription()
        const localDescription = this.peer.localDescription
        if (!localDescription) {
            console.error(`[WebRTC]: Failed to set local description in OnNegotiationNeeded`)
            this.logger?.debug("Failed to set local description in OnNegotiationNeeded")
            return
        }

        console.info(`[WebRTC]: Sending local description (${localDescription.type})`)
        this.logger?.debug(`OnNegotiationNeeded: Sending local description: ${localDescription.type}`)
        this.sendMessage({
            Description: {
                ty: localDescription.type,
                sdp: localDescription.sdp ?? ""
            }
        })
    }

    private remoteDescription: RTCSessionDescriptionInit | null = null
    private async handleRemoteDescription(sdp: RTCSessionDescriptionInit | null) {
        this.logger?.debug(`Received remote description: ${sdp?.type}`)

        this.remoteDescription = sdp
        if (!this.peer) {
            console.warn(`[WebRTC]: Cannot handle remote description - no peer connection`)
            return
        }

        if (this.remoteDescription) {
            try {
                console.info(`[WebRTC]: Setting remote description (${this.remoteDescription.type})`)
                await this.peer.setRemoteDescription(this.remoteDescription)
                console.info(`[WebRTC]: Remote description set successfully`)
            } catch (err) {
                console.error(`[WebRTC]: Failed to set remote description:`, err)
                return
            }

            if (this.remoteDescription.type == "offer") {
                try {
                    await this.peer.setLocalDescription()
                } catch (err) {
                    console.error(`[WebRTC]: Failed to create answer:`, err)
                    return
                }
                const localDescription = this.peer.localDescription
                if (!localDescription) {
                    console.error(`[WebRTC]: Peer didn't have a localDescription whilst receiving an offer and trying to answer`)
                    this.logger?.debug("Peer didn't have a localDescription whilst receiving an offer and trying to answer")
                    return
                }

                console.info(`[WebRTC]: Sending answer`)
                this.logger?.debug(`Responding to offer description: ${localDescription.type}`)
                this.sendMessage({
                    Description: {
                        ty: localDescription.type,
                        sdp: localDescription.sdp ?? ""
                    }
                })
            }

            this.remoteDescription = null
        }
    }

    private onIceCandidate(event: RTCPeerConnectionIceEvent) {
        if (event.candidate) {
            const candidate = event.candidate.toJSON()
            this.logger?.debug(`Sending ice candidate: ${candidate.candidate}`)

            this.sendMessage({
                AddIceCandidate: {
                    candidate: candidate.candidate ?? "",
                    sdp_mid: candidate.sdpMid ?? null,
                    sdp_mline_index: candidate.sdpMLineIndex ?? null,
                    username_fragment: candidate.usernameFragment ?? null
                }
            })
        } else {
            this.logger?.debug("No new ice candidates")
        }
    }

    private iceCandidates: Array<RTCIceCandidateInit> = []
    private async addIceCandidate(candidate: RTCIceCandidateInit) {
        this.logger?.debug(`Received ice candidate: ${candidate.candidate}`)

        if (!this.peer) {
            this.logger?.debug("Buffering ice candidate")

            this.iceCandidates.push(candidate)
            return
        }
        await this.tryDequeueIceCandidates()

        await this.peer.addIceCandidate(candidate)
    }
    private async tryDequeueIceCandidates() {
        if (!this.peer) {
            this.logger?.debug("called tryDequeueIceCandidates without a peer")
            return
        }

        for (const candidate of this.iceCandidates) {
            await this.peer.addIceCandidate(candidate)
        }
        this.iceCandidates.length = 0
    }

    private onConnectionStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnConnectionStateChange without a peer")
            return
        }

        let type: null | "fatal" | "recover" = null

        if (this.peer.connectionState == "connected") {
            type = "recover"
            this.applyDelayHintsToReceivers()

            if (this.onconnected) {
                this.onconnected()
            }
        } else if ((this.peer.connectionState == "failed" || this.peer.connectionState == "closed") && this.peer.iceGatheringState == "complete") {
            type = "fatal"
        }

        // Always log connection state changes to console for debugging
        console.info(`[WebRTC]: Connection state: ${this.peer.connectionState}, ICE gathering: ${this.peer.iceGatheringState}`)
        this.logger?.debug(`Changing Peer State to ${this.peer.connectionState}`, {
            type: type ?? undefined
        })
    }
    private onIceConnectionStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnIceConnectionStateChange without a peer")
            return
        }
        // Always log ICE state changes to console for debugging
        console.info(`[WebRTC]: ICE connection state: ${this.peer.iceConnectionState}`)
        this.logger?.debug(`Changing Peer Ice State to ${this.peer.iceConnectionState}`)
    }
    private onIceGatheringStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnIceGatheringStateChange without a peer")
            return
        }
        this.logger?.debug(`Changing Peer Ice Gathering State to ${this.peer.iceGatheringState}`)
    }

    private applyDelayHints(receiver: RTCRtpReceiver, trackKind: string) {
        if ("playoutDelayHint" in receiver) {
            // Ask the browser to keep playout delay as low as possible.
            receiver.playoutDelayHint = 0
        }

        if (trackKind !== "video") {
            return
        }

        // Be more aggressive only for video. Forcing audio too hard can create
        // audible artifacts, so we leave audio buffering policy to the browser.
        if ("jitterBufferTarget" in receiver) {
            receiver.jitterBufferTarget = 0
        }
        if ("jitterBufferDelayHint" in receiver) {
            receiver.jitterBufferDelayHint = 0
        }
    }

    private applyDelayHintsToReceivers() {
        if (!this.peer) {
            return
        }

        for (const receiver of this.peer.getReceivers()) {
            this.applyDelayHints(receiver, receiver.track?.kind ?? "")
        }
    }

    private channels: Array<TransportChannel | null> = []
    private initChannels() {
        if (!this.peer) {
            this.logger?.debug("Failed to initialize channel without peer")
            return
        }
        if (this.channels.length > 0) {
            this.logger?.debug("Already initialized channels")
            return
        }

        for (const channelRaw in TRANSPORT_CHANNEL_OPTIONS) {
            const channel = channelRaw as TransportChannelIdKey
            const options = TRANSPORT_CHANNEL_OPTIONS[channel]

            if (channel == "HOST_VIDEO") {
                const channel: VideoTrackTransportChannel = new WebRTCInboundTrackTransportChannel<"videotrack">(this.logger, "videotrack", "video", this.videoTrackHolder)
                this.channels[TransportChannelId.HOST_VIDEO] = channel
                continue
            }
            if (channel == "HOST_AUDIO") {
                const channel: AudioTrackTransportChannel = new WebRTCInboundTrackTransportChannel<"audiotrack">(this.logger, "audiotrack", "audio", this.audioTrackHolder)
                this.channels[TransportChannelId.HOST_AUDIO] = channel
                continue
            }

            const id = TransportChannelId[channel]
            const transportChannel = new WebRTCDataTransportChannel(channel)
            this.channels[id] = transportChannel

            if (this.negotiationRole === "clientoffer") {
                const dataChannel = this.peer.createDataChannel(channel.toLowerCase(), {
                    // TODO: use id
                    // id,
                    // negotiated: true,
                    ordered: options.ordered,
                    maxRetransmits: options.reliable ? undefined : 0
                })
                transportChannel.bindChannel(dataChannel)
            }
        }
    }

    private onDataChannel(event: RTCDataChannelEvent) {
        const dataChannel = event.channel
        const channelId = this.getDataChannelIdForLabel(dataChannel.label)
        if (channelId == null) {
            console.warn(`[WebRTC]: Received unexpected data channel "${dataChannel.label}"`)
            return
        }

        const channel = this.channels[channelId]
        if (!channel || channel.type !== "data") {
            console.warn(`[WebRTC]: Received data channel "${dataChannel.label}" before transport channel placeholder existed`)
            return
        }

        ;(channel as WebRTCDataTransportChannel).bindChannel(dataChannel)
    }

    private getDataChannelIdForLabel(label: string): TransportChannelIdValue | null {
        for (const channelRaw in TRANSPORT_CHANNEL_OPTIONS) {
            const channel = channelRaw as TransportChannelIdKey
            if (channel === "HOST_VIDEO" || channel === "HOST_AUDIO") {
                continue
            }
            if (channel.toLowerCase() === label) {
                return TransportChannelId[channel]
            }
        }

        return null
    }

    private videoTrackHolder: TrackHolder = { ontrack: null, track: null }
    private videoReceiver: RTCRtpReceiver | null = null

    private audioTrackHolder: TrackHolder = { ontrack: null, track: null }

    private onTrack(event: RTCTrackEvent) {
        const track = event.track

        console.info(`[WebRTC]: Received track: kind=${track.kind}, id=${track.id}, label=${track.label}`)

        this.applyDelayHints(event.receiver, track.kind)

        if (track.kind == "video") {
            this.videoReceiver = event.receiver
        }

        this.logger?.debug(`Adding receiver: ${track.kind}, ${track.id}, ${track.label}`)

        if (track.kind == "video") {
            if ("contentHint" in track) {
                track.contentHint = "motion"
            }

            console.info(`[WebRTC]: Video track received and configured`)
            this.videoTrackHolder.track = track
            if (!this.videoTrackHolder.ontrack) {
                console.warn("[WebRTC]: Video track received before channel handler was ready")
                return
            }
            this.videoTrackHolder.ontrack()
        } else if (track.kind == "audio") {
            console.info(`[WebRTC]: Audio track received`)
            this.audioTrackHolder.track = track
            if (!this.audioTrackHolder.ontrack) {
                console.warn("[WebRTC]: Audio track received before channel handler was ready")
                return
            }
            this.audioTrackHolder.ontrack()
        }
    }

    async setupHostVideo(_setup: TransportVideoSetup): Promise<void> {
        // TODO: check transport type
    }

    async setupHostAudio(_setup: TransportAudioSetup): Promise<void> {
        // TODO: check transport type
    }

    getChannel(id: TransportChannelIdValue): TransportChannel {
        const channel = this.channels[id]
        if (!channel) {
            this.logger?.debug("Failed to setup video without peer")
            throw `Failed to get channel because it is not yet initialized, Id: ${id}`
        }

        return channel
    }

    onconnected: (() => void) | null = null
    ondisconnected: (() => void) | null = null

    onclose: (() => void) | null = null
    async close(): Promise<void> {
        this.previousVideoStatsSample = null
        this.peer?.close()
    }

    async getConnectionInfo(): Promise<{ connectionType: string, isRelay: boolean, rttMs: number }> {
        if (!this.peer) {
            return { connectionType: "unknown", isRelay: false, rttMs: -1 }
        }

        try {
            const stats = await this.peer.getStats()
            for (const [, value] of stats.entries()) {
                if (value.type === "candidate-pair" && value.state === "succeeded") {
                    const localCandidateId = value.localCandidateId
                    const rttMs = value.currentRoundTripTime ? value.currentRoundTripTime * 1000 : -1
                    
                    // Find the local candidate to determine connection type
                    for (const [, candidate] of stats.entries()) {
                        if (candidate.type === "local-candidate" && candidate.id === localCandidateId) {
                            const candidateType = candidate.candidateType || "unknown"
                            const isRelay = candidateType === "relay"
                            let connectionType = "unknown"
                            
                            if (isRelay) {
                                connectionType = "relay"
                            } else if (candidateType === "host") {
                                connectionType = "lan"
                            } else {
                                connectionType = "wan"
                            }
                            
                            return { connectionType, isRelay, rttMs }
                        }
                    }
                }
            }
        } catch (e) {
            console.warn("[WebRTC]: Failed to get connection info", e)
        }
        
        return { connectionType: "unknown", isRelay: false, rttMs: -1 }
    }

    async getStats(): Promise<Record<string, string>> {
        const statsData: Record<string, string> = {}

        if (!this.videoReceiver) {
            return {}
        }
        const stats = await this.videoReceiver.getStats()

        // Also get connection-level stats for RTT
        if (this.peer) {
            try {
                const peerStats = await this.peer.getStats()
                for (const [, value] of peerStats.entries()) {
                    if (value.type === "candidate-pair" && value.state === "succeeded") {
                        // currentRoundTripTime is in SECONDS, convert to ms
                        if (value.currentRoundTripTime != null) {
                            statsData.webrtcRttMs = (value.currentRoundTripTime * 1000).toString()
                        }
                    }
                }
            } catch (e) {
                console.debug("[WebRTC]: Failed to get peer stats for RTT", e)
            }
        }

        // Collect raw cumulative values. WebRTC reports these as totals since the
        // receiver started, so we convert them into per-interval averages below.
        let jitterBufferDelay = 0      // cumulative seconds
        let jitterBufferTargetDelay = 0
        let jitterBufferMinimumDelay = 0
        let jitterBufferEmittedCount = 0
        let totalDecodeTime = 0        // cumulative seconds
        let framesDecoded = 0
        let totalProcessingDelay = 0   // cumulative seconds

        for (const [key, value] of stats.entries()) {
            // Decoder info
            if ("decoderImplementation" in value && value.decoderImplementation != null) {
                statsData.decoderImplementation = value.decoderImplementation
            }
            if ("frameWidth" in value && value.frameWidth != null) {
                statsData.videoWidth = value.frameWidth
            }
            if ("frameHeight" in value && value.frameHeight != null) {
                statsData.videoHeight = value.frameHeight
            }
            if ("framesPerSecond" in value && value.framesPerSecond != null) {
                statsData.webrtcFps = value.framesPerSecond
            }

            // Cumulative values (in seconds) - we'll calculate averages below
            if ("jitterBufferDelay" in value && value.jitterBufferDelay != null) {
                jitterBufferDelay = value.jitterBufferDelay
            }
            if ("jitterBufferEmittedCount" in value && value.jitterBufferEmittedCount != null) {
                jitterBufferEmittedCount = value.jitterBufferEmittedCount
            }
            if ("jitterBufferTargetDelay" in value && value.jitterBufferTargetDelay != null) {
                jitterBufferTargetDelay = value.jitterBufferTargetDelay
            }
            if ("jitterBufferMinimumDelay" in value && value.jitterBufferMinimumDelay != null) {
                jitterBufferMinimumDelay = value.jitterBufferMinimumDelay
            }
            if ("totalDecodeTime" in value && value.totalDecodeTime != null) {
                totalDecodeTime = value.totalDecodeTime
            }
            if ("framesDecoded" in value && value.framesDecoded != null) {
                framesDecoded = value.framesDecoded
            }
            if ("totalProcessingDelay" in value && value.totalProcessingDelay != null) {
                totalProcessingDelay = value.totalProcessingDelay
            }

            // Jitter is in SECONDS, store raw value (will convert in stats.ts)
            if ("jitter" in value && value.jitter != null) {
                statsData.webrtcJitterSec = value.jitter.toString()
            }

            // Packet stats
            if ("packetsReceived" in value && value.packetsReceived != null) {
                statsData.webrtcPacketsReceived = value.packetsReceived
            }
            if ("packetsLost" in value && value.packetsLost != null) {
                statsData.webrtcPacketsLost = value.packetsLost
            }
            if ("framesDropped" in value && value.framesDropped != null) {
                statsData.webrtcFramesDropped = value.framesDropped
            }
        }

        const previousSample = this.previousVideoStatsSample
        this.previousVideoStatsSample = {
            jitterBufferDelay,
            jitterBufferTargetDelay,
            jitterBufferMinimumDelay,
            jitterBufferEmittedCount,
            totalDecodeTime,
            totalProcessingDelay,
            framesDecoded,
        }

        // Report interval-based averages instead of lifetime averages. The raw
        // counters monotonically increase, so using the full totals makes the UI
        // "buffer" value climb slowly toward the true steady-state delay.
        if (previousSample) {
            const emittedDelta = jitterBufferEmittedCount - previousSample.jitterBufferEmittedCount
            const jitterDelayDelta = jitterBufferDelay - previousSample.jitterBufferDelay
            if (emittedDelta > 0 && jitterDelayDelta >= 0) {
                const avgJitterBufferDelayMs = (jitterDelayDelta / emittedDelta) * 1000
                statsData.webrtcAvgJitterBufferDelayMs = avgJitterBufferDelayMs.toString()
            }

            const jitterTargetDelayDelta = jitterBufferTargetDelay - previousSample.jitterBufferTargetDelay
            if (emittedDelta > 0 && jitterTargetDelayDelta >= 0) {
                const avgJitterBufferTargetDelayMs = (jitterTargetDelayDelta / emittedDelta) * 1000
                statsData.webrtcJitterBufferTargetDelayMs = avgJitterBufferTargetDelayMs.toString()
            }

            const jitterMinimumDelayDelta = jitterBufferMinimumDelay - previousSample.jitterBufferMinimumDelay
            if (emittedDelta > 0 && jitterMinimumDelayDelta >= 0) {
                const avgJitterBufferMinimumDelayMs = (jitterMinimumDelayDelta / emittedDelta) * 1000
                statsData.webrtcJitterBufferMinimumDelayMs = avgJitterBufferMinimumDelayMs.toString()
            }

            const decodedDelta = framesDecoded - previousSample.framesDecoded
            const decodeTimeDelta = totalDecodeTime - previousSample.totalDecodeTime
            if (decodedDelta > 0 && decodeTimeDelta >= 0) {
                const avgDecodeTimeMs = (decodeTimeDelta / decodedDelta) * 1000
                statsData.webrtcAvgDecodeTimeMs = avgDecodeTimeMs.toString()
            }

            const processingDelayDelta = totalProcessingDelay - previousSample.totalProcessingDelay
            if (decodedDelta > 0 && processingDelayDelta >= 0) {
                const avgProcessingDelayMs = (processingDelayDelta / decodedDelta) * 1000
                statsData.webrtcAvgProcessingDelayMs = avgProcessingDelayMs.toString()
            }
        }

        return statsData
    }
}

type TrackHolder = {
    ontrack: (() => void) | null
    track: MediaStreamTrack | null
}

// This receives track data
class WebRTCInboundTrackTransportChannel<T extends string> implements TrackTransportChannel {
    type: T

    canReceive: boolean = true
    canSend: boolean = false

    private logger: Logger | null

    private label: string
    private trackHolder: TrackHolder

    constructor(logger: Logger | null, type: T, label: string, trackHolder: TrackHolder) {
        this.logger = logger

        this.type = type
        this.label = label
        this.trackHolder = trackHolder

        this.trackHolder.ontrack = this.onTrack.bind(this)
    }
    setTrack(_track: MediaStreamTrack | null): void {
        throw "WebRTCInboundTrackTransportChannel cannot addTrack"
    }

    private onTrack() {
        const track = this.trackHolder.track
        if (!track) {
            this.logger?.debug("WebRTC TrackHolder.track is null!")
            return
        }

        console.info(`[WebRTC-Channel]: onTrack called for ${this.label}, listeners count: ${this.trackListeners.size}`)
        for (const [listener, lastTrack] of this.trackListeners.entries()) {
            if (lastTrack === track) {
                continue
            }

            listener(track)
            this.trackListeners.set(listener, track)
        }
    }


    private trackListeners: Map<(track: MediaStreamTrack) => void, MediaStreamTrack | null> = new Map()
    addTrackListener(listener: (track: MediaStreamTrack) => void): void {
        console.info(`[WebRTC-Channel]: addTrackListener called for ${this.label}, track already exists: ${!!this.trackHolder.track}`)
        this.trackListeners.set(listener, this.trackHolder.track)
        if (this.trackHolder.track) {
            console.info(`[WebRTC-Channel]: Calling listener immediately with existing track for ${this.label}`)
            listener(this.trackHolder.track)
        }
    }
    removeTrackListener(listener: (track: MediaStreamTrack) => void): void {
        this.trackListeners.delete(listener)
    }
}

class WebRTCDataTransportChannel implements DataTransportChannel {
    type: "data" = "data"

    canReceive: boolean = true
    canSend: boolean = true

    private label: string
    private channel: RTCDataChannel | null = null

    constructor(label: string, channel?: RTCDataChannel) {
        this.label = label
        if (channel) {
            this.bindChannel(channel)
        }
    }

    bindChannel(channel: RTCDataChannel) {
        if (this.channel === channel) {
            return
        }
        if (this.channel) {
            console.warn(`[WebRTC]: Data channel "${this.label}" was already bound; ignoring duplicate binding`)
            return
        }

        this.channel = channel
        this.channel.addEventListener("message", this.onMessage.bind(this))
        this.channel.addEventListener("open", this.onOpen.bind(this))

        if (this.channel.readyState === "open") {
            this.tryDequeueSendQueue()
        }
    }

    private onOpen() {
        this.tryDequeueSendQueue()
    }

    private sendQueue: Array<ArrayBuffer> = []
    send(message: ArrayBuffer): void {
        console.debug(this.label, message)

        if (!this.channel || this.channel.readyState != "open") {
            const readyState = this.channel?.readyState ?? "unbound"
            console.debug(`Tried sending packet to ${this.label} with readyState ${readyState}. Buffering it for the future.`)
            this.sendQueue.push(message)
        } else {
            this.tryDequeueSendQueue()
            this.channel.send(message)
        }
    }
    private tryDequeueSendQueue() {
        if (!this.channel || this.channel.readyState != "open") {
            return
        }

        for (const message of this.sendQueue) {
            this.channel.send(message)
        }
        this.sendQueue.length = 0
    }

    private onMessage(event: MessageEvent) {
        const data = event.data
        if (!(data instanceof ArrayBuffer)) {
            console.warn(`received text data on webrtc channel ${this.label}`)
            return
        }

        for (const listener of this.receiveListeners) {
            listener(event.data)
        }
    }
    private receiveListeners: Array<(data: ArrayBuffer) => void> = []
    addReceiveListener(listener: (data: ArrayBuffer) => void): void {
        this.receiveListeners.push(listener)
    }
    removeReceiveListener(listener: (data: ArrayBuffer) => void): void {
        const index = this.receiveListeners.indexOf(listener)
        if (index != -1) {
            this.receiveListeners.splice(index, 1)
        }
    }
    estimatedBufferedBytes(): number {
        return this.channel?.bufferedAmount ?? 0
    }
}
