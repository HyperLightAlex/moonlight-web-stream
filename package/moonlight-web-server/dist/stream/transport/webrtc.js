var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
import { TransportChannelId } from "../../api_bindings.js";
import { TRANSPORT_CHANNEL_OPTIONS } from "./index.js";
export class WebRTCTransport {
    constructor(logger) {
        this.implementationName = "webrtc";
        this.peer = null;
        this.onsendmessage = null;
        this.remoteDescription = null;
        this.iceCandidates = [];
        this.forceDelayInterval = null;
        this.channels = [];
        this.videoTrackHolder = { ontrack: null, track: null };
        this.videoReceiver = null;
        this.audioTrackHolder = { ontrack: null, track: null };
        this.onconnected = null;
        this.ondisconnected = null;
        this.onclose = null;
        this.logger = logger !== null && logger !== void 0 ? logger : null;
    }
    initPeer(configuration) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Creating Client Peer`);
            if (this.peer) {
                (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug(`Cannot create Peer because a Peer already exists`);
                return;
            }
            // Configure web rtc
            this.peer = new RTCPeerConnection(configuration);
            this.peer.addEventListener("error", this.onError.bind(this));
            this.peer.addEventListener("negotiationneeded", this.onNegotiationNeeded.bind(this));
            this.peer.addEventListener("icecandidate", this.onIceCandidate.bind(this));
            this.peer.addEventListener("connectionstatechange", this.onConnectionStateChange.bind(this));
            this.peer.addEventListener("iceconnectionstatechange", this.onIceConnectionStateChange.bind(this));
            this.peer.addEventListener("icegatheringstatechange", this.onIceGatheringStateChange.bind(this));
            this.peer.addEventListener("track", this.onTrack.bind(this));
            this.initChannels();
            // Maybe we already received data
            if (this.remoteDescription) {
                yield this.handleRemoteDescription(this.remoteDescription);
            }
            else {
                yield this.onNegotiationNeeded();
            }
            yield this.tryDequeueIceCandidates();
        });
    }
    onError(event) {
        var _a;
        (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Web Socket or WebRtcPeer Error`);
        console.error(`Web Socket or WebRtcPeer Error`, event);
    }
    sendMessage(message) {
        var _a;
        if (this.onsendmessage) {
            this.onsendmessage(message);
        }
        else {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("Failed to call onicecandidate because no handler is set");
        }
    }
    onReceiveMessage(message) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            if ("Description" in message) {
                const description = message.Description;
                console.info(`[WebRTC]: Received remote description type: ${description.ty}`);
                yield this.handleRemoteDescription({
                    type: description.ty,
                    sdp: description.sdp
                });
            }
            else if ("AddIceCandidate" in message) {
                const candidate = message.AddIceCandidate;
                console.info(`[WebRTC]: Received ICE candidate: ${(_a = candidate.candidate) === null || _a === void 0 ? void 0 : _a.substring(0, 50)}...`);
                yield this.addIceCandidate({
                    candidate: candidate.candidate,
                    sdpMid: candidate.sdp_mid,
                    sdpMLineIndex: candidate.sdp_mline_index,
                    usernameFragment: candidate.username_fragment
                });
            }
        });
    }
    onNegotiationNeeded() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b, _c, _d;
            // We're polite
            if (!this.peer) {
                (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("OnNegotiationNeeded without a peer");
                return;
            }
            console.info(`[WebRTC]: Negotiation needed, creating local description...`);
            yield this.peer.setLocalDescription();
            const localDescription = this.peer.localDescription;
            if (!localDescription) {
                console.error(`[WebRTC]: Failed to set local description in OnNegotiationNeeded`);
                (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug("Failed to set local description in OnNegotiationNeeded");
                return;
            }
            console.info(`[WebRTC]: Sending local description (${localDescription.type})`);
            (_c = this.logger) === null || _c === void 0 ? void 0 : _c.debug(`OnNegotiationNeeded: Sending local description: ${localDescription.type}`);
            this.sendMessage({
                Description: {
                    ty: localDescription.type,
                    sdp: (_d = localDescription.sdp) !== null && _d !== void 0 ? _d : ""
                }
            });
        });
    }
    handleRemoteDescription(sdp) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b, _c, _d;
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Received remote description: ${sdp === null || sdp === void 0 ? void 0 : sdp.type}`);
            this.remoteDescription = sdp;
            if (!this.peer) {
                console.warn(`[WebRTC]: Cannot handle remote description - no peer connection`);
                return;
            }
            if (this.remoteDescription) {
                try {
                    console.info(`[WebRTC]: Setting remote description (${this.remoteDescription.type})`);
                    yield this.peer.setRemoteDescription(this.remoteDescription);
                    console.info(`[WebRTC]: Remote description set successfully`);
                }
                catch (err) {
                    console.error(`[WebRTC]: Failed to set remote description:`, err);
                    return;
                }
                if (this.remoteDescription.type == "offer") {
                    try {
                        yield this.peer.setLocalDescription();
                    }
                    catch (err) {
                        console.error(`[WebRTC]: Failed to create answer:`, err);
                        return;
                    }
                    const localDescription = this.peer.localDescription;
                    if (!localDescription) {
                        console.error(`[WebRTC]: Peer didn't have a localDescription whilst receiving an offer and trying to answer`);
                        (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug("Peer didn't have a localDescription whilst receiving an offer and trying to answer");
                        return;
                    }
                    console.info(`[WebRTC]: Sending answer`);
                    (_c = this.logger) === null || _c === void 0 ? void 0 : _c.debug(`Responding to offer description: ${localDescription.type}`);
                    this.sendMessage({
                        Description: {
                            ty: localDescription.type,
                            sdp: (_d = localDescription.sdp) !== null && _d !== void 0 ? _d : ""
                        }
                    });
                }
                this.remoteDescription = null;
            }
        });
    }
    onIceCandidate(event) {
        var _a, _b, _c, _d, _e, _f;
        if (event.candidate) {
            const candidate = event.candidate.toJSON();
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Sending ice candidate: ${candidate.candidate}`);
            this.sendMessage({
                AddIceCandidate: {
                    candidate: (_b = candidate.candidate) !== null && _b !== void 0 ? _b : "",
                    sdp_mid: (_c = candidate.sdpMid) !== null && _c !== void 0 ? _c : null,
                    sdp_mline_index: (_d = candidate.sdpMLineIndex) !== null && _d !== void 0 ? _d : null,
                    username_fragment: (_e = candidate.usernameFragment) !== null && _e !== void 0 ? _e : null
                }
            });
        }
        else {
            (_f = this.logger) === null || _f === void 0 ? void 0 : _f.debug("No new ice candidates");
        }
    }
    addIceCandidate(candidate) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Received ice candidate: ${candidate.candidate}`);
            if (!this.peer) {
                (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug("Buffering ice candidate");
                this.iceCandidates.push(candidate);
                return;
            }
            yield this.tryDequeueIceCandidates();
            yield this.peer.addIceCandidate(candidate);
        });
    }
    tryDequeueIceCandidates() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            if (!this.peer) {
                (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("called tryDequeueIceCandidates without a peer");
                return;
            }
            for (const candidate of this.iceCandidates) {
                yield this.peer.addIceCandidate(candidate);
            }
            this.iceCandidates.length = 0;
        });
    }
    onConnectionStateChange() {
        var _a, _b;
        if (!this.peer) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("OnConnectionStateChange without a peer");
            return;
        }
        let type = null;
        if (this.peer.connectionState == "connected") {
            type = "recover";
            this.setDelayHintInterval(true);
        }
        else if ((this.peer.connectionState == "failed" || this.peer.connectionState == "disconnected") && this.peer.iceGatheringState == "complete") {
            type = "fatal";
            this.setDelayHintInterval(false);
        }
        // Always log connection state changes to console for debugging
        console.info(`[WebRTC]: Connection state: ${this.peer.connectionState}, ICE gathering: ${this.peer.iceGatheringState}`);
        (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug(`Changing Peer State to ${this.peer.connectionState}`, {
            type: type !== null && type !== void 0 ? type : undefined
        });
    }
    onIceConnectionStateChange() {
        var _a, _b;
        if (!this.peer) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("OnIceConnectionStateChange without a peer");
            return;
        }
        // Always log ICE state changes to console for debugging
        console.info(`[WebRTC]: ICE connection state: ${this.peer.iceConnectionState}`);
        (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug(`Changing Peer Ice State to ${this.peer.iceConnectionState}`);
    }
    onIceGatheringStateChange() {
        var _a, _b;
        if (!this.peer) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("OnIceGatheringStateChange without a peer");
            return;
        }
        (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug(`Changing Peer Ice Gathering State to ${this.peer.iceGatheringState}`);
    }
    setDelayHintInterval(setRunning) {
        if (this.forceDelayInterval == null && setRunning) {
            this.forceDelayInterval = setInterval(() => {
                if (!this.peer) {
                    return;
                }
                for (const receiver of this.peer.getReceivers()) {
                    // @ts-ignore
                    receiver.jitterBufferTarget = receiver.jitterBufferDelayHint = receiver.playoutDelayHint = 0;
                }
            }, 15);
        }
        else if (this.forceDelayInterval != null && !setRunning) {
            clearInterval(this.forceDelayInterval);
        }
    }
    initChannels() {
        var _a, _b;
        if (!this.peer) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("Failed to initialize channel without peer");
            return;
        }
        if (this.channels.length > 0) {
            (_b = this.logger) === null || _b === void 0 ? void 0 : _b.debug("Already initialized channels");
            return;
        }
        for (const channelRaw in TRANSPORT_CHANNEL_OPTIONS) {
            const channel = channelRaw;
            const options = TRANSPORT_CHANNEL_OPTIONS[channel];
            if (channel == "HOST_VIDEO") {
                const channel = new WebRTCInboundTrackTransportChannel(this.logger, "videotrack", "video", this.videoTrackHolder);
                this.channels[TransportChannelId.HOST_VIDEO] = channel;
                continue;
            }
            if (channel == "HOST_AUDIO") {
                const channel = new WebRTCInboundTrackTransportChannel(this.logger, "audiotrack", "audio", this.audioTrackHolder);
                this.channels[TransportChannelId.HOST_AUDIO] = channel;
                continue;
            }
            const id = TransportChannelId[channel];
            const dataChannel = this.peer.createDataChannel(channel.toLowerCase(), {
                // TODO: use id
                // id,
                // negotiated: true,
                ordered: options.ordered,
                maxRetransmits: options.reliable ? undefined : 0
            });
            this.channels[id] = new WebRTCDataTransportChannel(channel, dataChannel);
        }
    }
    onTrack(event) {
        var _a;
        const track = event.track;
        console.info(`[WebRTC]: Received track: kind=${track.kind}, id=${track.id}, label=${track.label}`);
        if (track.kind == "video") {
            this.videoReceiver = event.receiver;
        }
        (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Adding receiver: ${track.kind}, ${track.id}, ${track.label}`);
        if (track.kind == "video") {
            if ("contentHint" in track) {
                track.contentHint = "motion";
            }
            console.info(`[WebRTC]: Video track received and configured`);
            this.videoTrackHolder.track = track;
            if (!this.videoTrackHolder.ontrack) {
                throw "No video track listener registered!";
            }
            this.videoTrackHolder.ontrack();
        }
        else if (track.kind == "audio") {
            console.info(`[WebRTC]: Audio track received`);
            this.audioTrackHolder.track = track;
            if (!this.audioTrackHolder.ontrack) {
                throw "No audio track listener registered!";
            }
            this.audioTrackHolder.ontrack();
        }
    }
    setupHostVideo(_setup) {
        return __awaiter(this, void 0, void 0, function* () {
            // TODO: check transport type
        });
    }
    setupHostAudio(_setup) {
        return __awaiter(this, void 0, void 0, function* () {
            // TODO: check transport type
        });
    }
    getChannel(id) {
        var _a;
        const channel = this.channels[id];
        if (!channel) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("Failed to setup video without peer");
            throw `Failed to get channel because it is not yet initialized, Id: ${id}`;
        }
        return channel;
    }
    close() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            (_a = this.peer) === null || _a === void 0 ? void 0 : _a.close();
        });
    }
    getConnectionInfo() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!this.peer) {
                return { connectionType: "unknown", isRelay: false, rttMs: -1 };
            }
            try {
                const stats = yield this.peer.getStats();
                for (const [, value] of stats.entries()) {
                    if (value.type === "candidate-pair" && value.state === "succeeded") {
                        const localCandidateId = value.localCandidateId;
                        const rttMs = value.currentRoundTripTime ? value.currentRoundTripTime * 1000 : -1;
                        // Find the local candidate to determine connection type
                        for (const [, candidate] of stats.entries()) {
                            if (candidate.type === "local-candidate" && candidate.id === localCandidateId) {
                                const candidateType = candidate.candidateType || "unknown";
                                const isRelay = candidateType === "relay";
                                let connectionType = "unknown";
                                if (isRelay) {
                                    connectionType = "relay";
                                }
                                else if (candidateType === "host") {
                                    connectionType = "lan";
                                }
                                else {
                                    connectionType = "wan";
                                }
                                return { connectionType, isRelay, rttMs };
                            }
                        }
                    }
                }
            }
            catch (e) {
                console.warn("[WebRTC]: Failed to get connection info", e);
            }
            return { connectionType: "unknown", isRelay: false, rttMs: -1 };
        });
    }
    getStats() {
        return __awaiter(this, void 0, void 0, function* () {
            const statsData = {};
            if (!this.videoReceiver) {
                return {};
            }
            const stats = yield this.videoReceiver.getStats();
            // Also get connection-level stats for RTT
            if (this.peer) {
                try {
                    const peerStats = yield this.peer.getStats();
                    for (const [, value] of peerStats.entries()) {
                        if (value.type === "candidate-pair" && value.state === "succeeded") {
                            // currentRoundTripTime is in SECONDS, convert to ms
                            if (value.currentRoundTripTime != null) {
                                statsData.webrtcRttMs = (value.currentRoundTripTime * 1000).toString();
                            }
                        }
                    }
                }
                catch (e) {
                    console.debug("[WebRTC]: Failed to get peer stats for RTT", e);
                }
            }
            // Collect raw values for calculating averages
            let jitterBufferDelay = 0; // cumulative seconds
            let jitterBufferEmittedCount = 0;
            let totalDecodeTime = 0; // cumulative seconds
            let framesDecoded = 0;
            let totalProcessingDelay = 0; // cumulative seconds
            for (const [key, value] of stats.entries()) {
                // Decoder info
                if ("decoderImplementation" in value && value.decoderImplementation != null) {
                    statsData.decoderImplementation = value.decoderImplementation;
                }
                if ("frameWidth" in value && value.frameWidth != null) {
                    statsData.videoWidth = value.frameWidth;
                }
                if ("frameHeight" in value && value.frameHeight != null) {
                    statsData.videoHeight = value.frameHeight;
                }
                if ("framesPerSecond" in value && value.framesPerSecond != null) {
                    statsData.webrtcFps = value.framesPerSecond;
                }
                // Cumulative values (in seconds) - we'll calculate averages below
                if ("jitterBufferDelay" in value && value.jitterBufferDelay != null) {
                    jitterBufferDelay = value.jitterBufferDelay;
                }
                if ("jitterBufferEmittedCount" in value && value.jitterBufferEmittedCount != null) {
                    jitterBufferEmittedCount = value.jitterBufferEmittedCount;
                }
                if ("totalDecodeTime" in value && value.totalDecodeTime != null) {
                    totalDecodeTime = value.totalDecodeTime;
                }
                if ("framesDecoded" in value && value.framesDecoded != null) {
                    framesDecoded = value.framesDecoded;
                }
                if ("totalProcessingDelay" in value && value.totalProcessingDelay != null) {
                    totalProcessingDelay = value.totalProcessingDelay;
                }
                // Jitter is in SECONDS, store raw value (will convert in stats.ts)
                if ("jitter" in value && value.jitter != null) {
                    statsData.webrtcJitterSec = value.jitter.toString();
                }
                // Packet stats
                if ("packetsReceived" in value && value.packetsReceived != null) {
                    statsData.webrtcPacketsReceived = value.packetsReceived;
                }
                if ("packetsLost" in value && value.packetsLost != null) {
                    statsData.webrtcPacketsLost = value.packetsLost;
                }
                if ("framesDropped" in value && value.framesDropped != null) {
                    statsData.webrtcFramesDropped = value.framesDropped;
                }
            }
            // Calculate per-frame averages and convert to milliseconds
            if (jitterBufferEmittedCount > 0) {
                const avgJitterBufferDelayMs = (jitterBufferDelay / jitterBufferEmittedCount) * 1000;
                statsData.webrtcAvgJitterBufferDelayMs = avgJitterBufferDelayMs.toString();
            }
            if (framesDecoded > 0) {
                const avgDecodeTimeMs = (totalDecodeTime / framesDecoded) * 1000;
                statsData.webrtcAvgDecodeTimeMs = avgDecodeTimeMs.toString();
                const avgProcessingDelayMs = (totalProcessingDelay / framesDecoded) * 1000;
                statsData.webrtcAvgProcessingDelayMs = avgProcessingDelayMs.toString();
            }
            return statsData;
        });
    }
}
// This receives track data
class WebRTCInboundTrackTransportChannel {
    constructor(logger, type, label, trackHolder) {
        this.canReceive = true;
        this.canSend = false;
        this.trackListeners = [];
        this.logger = logger;
        this.type = type;
        this.label = label;
        this.trackHolder = trackHolder;
        this.trackHolder.ontrack = this.onTrack.bind(this);
    }
    setTrack(_track) {
        throw "WebRTCInboundTrackTransportChannel cannot addTrack";
    }
    onTrack() {
        var _a;
        const track = this.trackHolder.track;
        if (!track) {
            (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug("WebRTC TrackHolder.track is null!");
            return;
        }
        console.info(`[WebRTC-Channel]: onTrack called for ${this.label}, listeners count: ${this.trackListeners.length}`);
        for (const listener of this.trackListeners) {
            listener(track);
        }
    }
    addTrackListener(listener) {
        console.info(`[WebRTC-Channel]: addTrackListener called for ${this.label}, track already exists: ${!!this.trackHolder.track}`);
        if (this.trackHolder.track) {
            console.info(`[WebRTC-Channel]: Calling listener immediately with existing track for ${this.label}`);
            listener(this.trackHolder.track);
        }
        this.trackListeners.push(listener);
    }
    removeTrackListener(listener) {
        const index = this.trackListeners.indexOf(listener);
        if (index != -1) {
            this.trackListeners.splice(index, 1);
        }
    }
}
class WebRTCDataTransportChannel {
    constructor(label, channel) {
        this.type = "data";
        this.canReceive = true;
        this.canSend = true;
        this.sendQueue = [];
        this.receiveListeners = [];
        this.label = label;
        this.channel = channel;
        this.channel.addEventListener("message", this.onMessage.bind(this));
    }
    send(message) {
        console.debug(this.label, message);
        if (this.channel.readyState != "open") {
            console.debug(`Tried sending packet to ${this.label} with readyState ${this.channel.readyState}. Buffering it for the future.`);
            this.sendQueue.push(message);
        }
        else {
            this.tryDequeueSendQueue();
            this.channel.send(message);
        }
    }
    tryDequeueSendQueue() {
        for (const message of this.sendQueue) {
            this.channel.send(message);
        }
        this.sendQueue.length = 0;
    }
    onMessage(event) {
        const data = event.data;
        if (!(data instanceof ArrayBuffer)) {
            console.warn(`received text data on webrtc channel ${this.label}`);
            return;
        }
        for (const listener of this.receiveListeners) {
            listener(event.data);
        }
    }
    addReceiveListener(listener) {
        this.receiveListeners.push(listener);
    }
    removeReceiveListener(listener) {
        const index = this.receiveListeners.indexOf(listener);
        if (index != -1) {
            this.receiveListeners.splice(index, 1);
        }
    }
    estimatedBufferedBytes() {
        return this.channel.bufferedAmount;
    }
}
