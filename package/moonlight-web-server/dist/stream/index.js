var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
import { TransportChannelId } from "../api_bindings.js";
import { AudioElementPlayer } from "./audio/audio_element.js";
import { defaultStreamInputConfig, StreamInput } from "./input.js";
import { Logger } from "./log.js";
import { StreamStats } from "./stats.js";
import { WebRTCTransport } from "./transport/webrtc.js";
import { createSupportedVideoFormatsBits, getSelectedVideoFormat } from "./video.js";
import { buildVideoPipeline } from "./video/pipeline.js";
export function getStreamerSize(settings, viewerScreenSize) {
    let width, height;
    if (settings.videoSize == "720p") {
        width = 1280;
        height = 720;
    }
    else if (settings.videoSize == "1080p") {
        width = 1920;
        height = 1080;
    }
    else if (settings.videoSize == "1440p") {
        width = 2560;
        height = 1440;
    }
    else if (settings.videoSize == "4k") {
        width = 3840;
        height = 2160;
    }
    else if (settings.videoSize == "custom") {
        width = settings.videoSizeCustom.width;
        height = settings.videoSizeCustom.height;
    }
    else { // native
        width = viewerScreenSize[0];
        height = viewerScreenSize[1];
    }
    return [width, height];
}
export class Stream {
    constructor(api, hostId, appId, settings, supportedVideoFormats, viewerScreenSize, hybridMode = false) {
        this.logger = new Logger();
        this.divElement = document.createElement("div");
        this.eventTarget = new EventTarget();
        this.iceServers = null;
        this.sessionToken = null;
        this.videoRenderer = null;
        this.audioPlayer = null;
        this.capabilities = null;
        this.transport = null;
        // -- Raw Web Socket stuff
        this.wsSendBuffer = [];
        this.logger.addInfoListener((info, type) => {
            this.debugLog(info, type !== null && type !== void 0 ? type : undefined);
        });
        this.api = api;
        this.hostId = hostId;
        this.appId = appId;
        this.hybridMode = hybridMode;
        this.settings = settings;
        this.streamerSize = getStreamerSize(settings, viewerScreenSize);
        // Configure web socket
        const wsApiHost = api.host_url.replace(/^http(s)?:/, "ws$1:");
        // TODO: firstly try out WebTransport
        this.ws = new WebSocket(`${wsApiHost}/host/stream`);
        this.ws.addEventListener("error", this.onError.bind(this));
        this.ws.addEventListener("open", this.onWsOpen.bind(this));
        this.ws.addEventListener("close", this.onWsClose.bind(this));
        this.ws.addEventListener("message", this.onRawWsMessage.bind(this));
        const fps = this.settings.fps;
        if (hybridMode) {
            this.debugLog("Hybrid mode enabled - input will be handled by native client");
        }
        this.sendWsMessage({
            Init: {
                host_id: this.hostId,
                app_id: this.appId,
                bitrate: this.settings.bitrate,
                packet_size: this.settings.packetSize,
                fps,
                width: this.streamerSize[0],
                height: this.streamerSize[1],
                video_frame_queue_size: this.settings.videoFrameQueueSize,
                play_audio_local: this.settings.playAudioLocal,
                audio_sample_queue_size: this.settings.audioSampleQueueSize,
                video_supported_formats: createSupportedVideoFormatsBits(supportedVideoFormats),
                video_colorspace: "Rec709", // TODO <---
                video_color_range_full: true, // TODO <---
                hybrid_mode: this.hybridMode,
            }
        });
        // Stream Input
        const streamInputConfig = defaultStreamInputConfig();
        Object.assign(streamInputConfig, {
            mouseScrollMode: this.settings.mouseScrollMode,
            controllerConfig: this.settings.controllerConfig
        });
        this.input = new StreamInput(streamInputConfig);
        // Stream Stats
        this.stats = new StreamStats();
        // Dispatch info for next frame so that listeners can be registers
        setTimeout(() => {
            this.debugLog("Requesting Stream with attributes: {");
            // Width, Height, Fps
            this.debugLog(`  Width ${this.streamerSize[0]}`);
            this.debugLog(`  Height ${this.streamerSize[1]}`);
            this.debugLog(`  Fps: ${fps}`);
            // Supported Video Formats
            const supportedVideoFormatsText = [];
            for (const item in supportedVideoFormats) {
                if (supportedVideoFormats[item]) {
                    supportedVideoFormatsText.push(item);
                }
            }
            this.debugLog(`  Supported Video Formats: ${createPrettyList(supportedVideoFormatsText)}`);
            this.debugLog("}");
        });
    }
    debugLog(message, type) {
        for (const line of message.split("\n")) {
            // Always log to console for debugging, especially in hybrid mode
            // where ConnectionInfoModal is not shown
            console.info(`[Stream]: ${line}`);
            const event = new CustomEvent("stream-info", {
                detail: { type: "addDebugLine", line, additional: type }
            });
            this.eventTarget.dispatchEvent(event);
        }
    }
    onMessage(message) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k, _l;
            if (typeof message == "string") {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "serverMessage", message }
                });
                this.eventTarget.dispatchEvent(event);
            }
            else if ("StageStarting" in message) {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "stageStarting", stage: message.StageStarting.stage }
                });
                this.eventTarget.dispatchEvent(event);
            }
            else if ("StageComplete" in message) {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "stageComplete", stage: message.StageComplete.stage }
                });
                this.eventTarget.dispatchEvent(event);
            }
            else if ("StageFailed" in message) {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "stageFailed", stage: message.StageFailed.stage, errorCode: message.StageFailed.error_code }
                });
                this.eventTarget.dispatchEvent(event);
                // Notify AndroidBridge of error
                const errorMsg = `Stage failed: ${message.StageFailed.stage} (error ${message.StageFailed.error_code})`;
                if ((_a = window.AndroidBridge) === null || _a === void 0 ? void 0 : _a.onStreamError) {
                    window.AndroidBridge.onStreamError(errorMsg);
                }
                window.dispatchEvent(new CustomEvent('streamError', {
                    detail: { message: errorMsg }
                }));
            }
            else if ("ConnectionTerminated" in message) {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "connectionTerminated", errorCode: message.ConnectionTerminated.error_code }
                });
                this.eventTarget.dispatchEvent(event);
                // Notify AndroidBridge of error
                const errorMsg = `Connection terminated (error ${message.ConnectionTerminated.error_code})`;
                if ((_b = window.AndroidBridge) === null || _b === void 0 ? void 0 : _b.onStreamError) {
                    window.AndroidBridge.onStreamError(errorMsg);
                }
                window.dispatchEvent(new CustomEvent('streamError', {
                    detail: { message: errorMsg }
                }));
            }
            else if ("UpdateApp" in message) {
                const event = new CustomEvent("stream-info", {
                    detail: { type: "app", app: message.UpdateApp.app }
                });
                this.eventTarget.dispatchEvent(event);
            }
            else if ("ConnectionComplete" in message) {
                const capabilities = message.ConnectionComplete.capabilities;
                this.capabilities = capabilities; // Store capabilities for MoonlightBridge API
                const formatRaw = message.ConnectionComplete.format;
                const width = message.ConnectionComplete.width;
                const height = message.ConnectionComplete.height;
                const fps = message.ConnectionComplete.fps;
                const format = getSelectedVideoFormat(formatRaw);
                if (format == null) {
                    this.debugLog(`Video Format ${formatRaw} was not found! Couldn't start stream!`, "fatal");
                    return;
                }
                const event = new CustomEvent("stream-info", {
                    detail: { type: "connectionComplete", capabilities }
                });
                this.eventTarget.dispatchEvent(event);
                // In hybrid mode, skip input setup as native client handles it
                if (!this.hybridMode) {
                    this.input.onStreamStart(capabilities, [width, height]);
                }
                this.stats.setVideoInfo(format !== null && format !== void 0 ? format : "Unknown", width, height, fps);
                yield Promise.all([
                    this.setupVideo({
                        format,
                        fps,
                        width,
                        height,
                    }),
                    // TODO: more audio info?
                    this.setupAudio()
                ]);
                this.debugLog(`Using video pipeline: ${(_c = this.transport) === null || _c === void 0 ? void 0 : _c.getChannel(TransportChannelId.HOST_VIDEO).type} (transport) -> ${(_d = this.videoRenderer) === null || _d === void 0 ? void 0 : _d.implementationName} (renderer)`);
                this.debugLog(`Using audio pipeline: ${(_e = this.transport) === null || _e === void 0 ? void 0 : _e.getChannel(TransportChannelId.HOST_AUDIO).type} (transport) -> ${(_f = this.audioPlayer) === null || _f === void 0 ? void 0 : _f.implementationName} (player)`);
                // In hybrid mode, auto-start video and audio since input is handled by native client
                // and we won't receive user interaction events in the WebView
                if (this.hybridMode) {
                    this.debugLog("Hybrid mode: auto-starting video and audio playback");
                    // Trigger the same actions as onUserInteraction() to unmute audio and ensure video plays
                    (_g = this.videoRenderer) === null || _g === void 0 ? void 0 : _g.onUserInteraction();
                    (_h = this.audioPlayer) === null || _h === void 0 ? void 0 : _h.onUserInteraction();
                }
                // Notify AndroidBridge if available (for hybrid mode)
                ;
                window.streamConnected = true;
                window.streamWidth = width;
                window.streamHeight = height;
                window.streamFps = fps;
                window.streamCodec = format;
                if ((_j = window.AndroidBridge) === null || _j === void 0 ? void 0 : _j.onStreamConnected) {
                    window.AndroidBridge.onStreamConnected(width, height, fps, format);
                }
                // Dispatch custom event for any listeners
                window.dispatchEvent(new CustomEvent('streamConnected', {
                    detail: { width, height, fps, codec: format }
                }));
            }
            // -- WebRTC Config
            else if ("Setup" in message) {
                const iceServers = message.Setup.ice_servers;
                const sessionToken = message.Setup.session_token;
                this.iceServers = iceServers;
                this.debugLog(`Using WebRTC Ice Servers: ${createPrettyList(iceServers.map(server => server.urls).reduce((list, url) => list.concat(url), []))}`);
                // Handle session token for hybrid mode
                if (sessionToken) {
                    this.sessionToken = sessionToken;
                    this.debugLog(`Hybrid mode session token received: ${sessionToken}`);
                    // Dispatch event for native bridge
                    const event = new CustomEvent("session-token", {
                        detail: { sessionToken }
                    });
                    this.eventTarget.dispatchEvent(event);
                    window.sessionToken = sessionToken;
                    // If AndroidBridge is available, notify it immediately
                    if ((_k = window.AndroidBridge) === null || _k === void 0 ? void 0 : _k.onSessionToken) {
                        window.AndroidBridge.onSessionToken(sessionToken);
                    }
                }
                yield this.startConnection();
            }
            // -- WebRTC
            else if ("WebRtc" in message) {
                const webrtcMessage = message.WebRtc;
                if (this.transport instanceof WebRTCTransport) {
                    this.transport.onReceiveMessage(webrtcMessage);
                }
                else {
                    this.debugLog(`Received WebRTC message but transport is currently ${(_l = this.transport) === null || _l === void 0 ? void 0 : _l.implementationName}`);
                }
            }
        });
    }
    startConnection() {
        return __awaiter(this, void 0, void 0, function* () {
            this.debugLog(`Using transport: ${this.settings.dataTransport}`);
            if (this.settings.dataTransport == "auto") {
                yield this.tryWebRTCTransport();
                yield this.tryWebSocketTransport();
            }
            else if (this.settings.dataTransport == "webrtc") {
                yield this.tryWebRTCTransport();
            }
            else if (this.settings.dataTransport == "websocket") {
                yield this.tryWebSocketTransport();
            }
            // TODO: handle failure
        });
    }
    setTransport(transport) {
        if (this.transport) {
            this.transport.close();
        }
        this.transport = transport;
        this.input.setTransport(this.transport);
        this.stats.setTransport(this.transport);
    }
    tryWebSocketTransport() {
        return __awaiter(this, void 0, void 0, function* () {
            this.debugLog("Trying Web Socket transport");
            // TODO
        });
    }
    tryWebRTCTransport() {
        return __awaiter(this, void 0, void 0, function* () {
            this.debugLog("Trying WebRTC transport");
            if (!this.iceServers) {
                this.debugLog(`Failed to try WebRTC Transport: no ice servers available`);
                return;
            }
            const transport = new WebRTCTransport(this.logger);
            transport.onsendmessage = (message) => this.sendWsMessage({ WebRtc: message });
            transport.initPeer({
                iceServers: this.iceServers
            });
            this.setTransport(transport);
        });
    }
    setupVideo(setup) {
        return __awaiter(this, void 0, void 0, function* () {
            if (this.videoRenderer) {
                this.debugLog("Found an old video renderer -> cleaning it up");
                this.videoRenderer.unmount(this.divElement);
                this.videoRenderer.cleanup();
                this.videoRenderer = null;
            }
            if (!this.transport) {
                this.debugLog("Failed to setup video without transport");
                return;
            }
            yield this.transport.setupHostVideo({
                type: ["videotrack"]
            });
            const video = this.transport.getChannel(TransportChannelId.HOST_VIDEO);
            if (video.type == "videotrack") {
                const { videoRenderer, log, error } = buildVideoPipeline("videotrack", this.settings);
                this.debugLog(log);
                if (error != null) {
                    this.debugLog(error, "fatal");
                    return;
                }
                videoRenderer.mount(this.divElement);
                videoRenderer.setup(setup);
                video.addTrackListener((track) => videoRenderer.setTrack(track));
                this.videoRenderer = videoRenderer;
            }
            else {
                this.debugLog(`Failed to create video pipeline with transport channel of type ${video.type} (${this.transport.implementationName})`);
                return;
            }
        });
    }
    setupAudio() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            if (this.audioPlayer) {
                this.debugLog("Found an old audio player -> cleaning it up");
                this.audioPlayer.unmount(this.divElement);
                this.audioPlayer.cleanup();
                this.audioPlayer = null;
            }
            if (!this.transport) {
                this.debugLog("Failed to setup audio without transport");
                return;
            }
            this.transport.setupHostAudio({
                type: ["audiotrack"]
            });
            const audio = (_a = this.transport) === null || _a === void 0 ? void 0 : _a.getChannel(TransportChannelId.HOST_AUDIO);
            if (audio.type == "audiotrack") {
                // TODO: create build audio pipeline fn
                if (AudioElementPlayer.isBrowserSupported()) {
                    const audioPlayer = new AudioElementPlayer();
                    audioPlayer.mount(this.divElement);
                    audioPlayer.setup({});
                    audio.addTrackListener((track) => audioPlayer.setTrack(track));
                    this.audioPlayer = audioPlayer;
                }
                else {
                    this.debugLog("No supported video renderer found!", "fatal");
                    return;
                }
            }
            else {
                throw "TODO";
            }
        });
    }
    mount(parent) {
        parent.appendChild(this.divElement);
    }
    unmount(parent) {
        parent.removeChild(this.divElement);
    }
    getVideoRenderer() {
        return this.videoRenderer;
    }
    getAudioPlayer() {
        return this.audioPlayer;
    }
    onWsOpen() {
        this.debugLog(`Web Socket Open`);
        for (const raw of this.wsSendBuffer.splice(0)) {
            this.ws.send(raw);
        }
    }
    onWsClose() {
        this.debugLog(`Web Socket Closed`);
    }
    onError(event) {
        this.debugLog(`Web Socket or WebRtcPeer Error`);
        console.error(`Web Socket or WebRtcPeer Error`, event);
    }
    sendWsMessage(message) {
        const raw = JSON.stringify(message);
        if (this.ws.readyState == WebSocket.OPEN) {
            this.ws.send(raw);
        }
        else {
            this.wsSendBuffer.push(raw);
        }
    }
    onRawWsMessage(event) {
        const message = event.data;
        if (typeof message == "string") {
            const json = JSON.parse(message);
            this.onMessage(json);
        }
        else if (message instanceof ArrayBuffer) {
            // TODO
        }
    }
    // -- Class Api
    addInfoListener(listener) {
        this.eventTarget.addEventListener("stream-info", listener);
    }
    removeInfoListener(listener) {
        this.eventTarget.removeEventListener("stream-info", listener);
    }
    addSessionTokenListener(listener) {
        this.eventTarget.addEventListener("session-token", listener);
    }
    removeSessionTokenListener(listener) {
        this.eventTarget.removeEventListener("session-token", listener);
    }
    getInput() {
        return this.input;
    }
    getStats() {
        return this.stats;
    }
    getStreamerSize() {
        return this.streamerSize;
    }
    getSessionToken() {
        return this.sessionToken;
    }
    isHybridMode() {
        return this.hybridMode;
    }
    getCapabilities() {
        return this.capabilities;
    }
    getStreamHealth() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b, _c, _d, _e, _f;
            const stats = this.stats.getCurrentStats();
            const connectionInfo = (_b = yield ((_a = this.transport) === null || _a === void 0 ? void 0 : _a.getConnectionInfo())) !== null && _b !== void 0 ? _b : { connectionType: "unknown", isRelay: false, rttMs: -1 };
            // Extract values with defaults
            const targetFps = (_c = stats.videoFps) !== null && _c !== void 0 ? _c : 60;
            const currentFps = stats.transport.webrtcFps ? parseFloat(stats.transport.webrtcFps) : -1;
            const hostLatencyMs = (_d = stats.avgHostProcessingLatencyMs) !== null && _d !== void 0 ? _d : -1;
            const streamerLatencyMs = (_e = stats.avgStreamerProcessingTimeMs) !== null && _e !== void 0 ? _e : -1;
            // Network RTT: prefer WebRTC stats (already in ms), fall back to streamer RTT
            const webrtcRttMs = stats.transport.webrtcRttMs ? parseFloat(stats.transport.webrtcRttMs) : -1;
            const networkRttMs = webrtcRttMs > 0 ? webrtcRttMs : (connectionInfo.rttMs > 0 ? connectionInfo.rttMs : ((_f = stats.streamerRttMs) !== null && _f !== void 0 ? _f : -1));
            const networkLatencyMs = networkRttMs > 0 ? networkRttMs / 2 : -1;
            // Decode latency from WebRTC stats (avg decode time, already in ms)
            const decodeLatencyMs = stats.transport.webrtcAvgDecodeTimeMs ? parseFloat(stats.transport.webrtcAvgDecodeTimeMs) : -1;
            // Calculate total latency (sum of available components)
            let totalLatencyMs = 0;
            let hasLatencyData = false;
            if (hostLatencyMs > 0) {
                totalLatencyMs += hostLatencyMs;
                hasLatencyData = true;
            }
            if (networkLatencyMs > 0) {
                totalLatencyMs += networkLatencyMs;
                hasLatencyData = true;
            }
            if (streamerLatencyMs > 0) {
                totalLatencyMs += streamerLatencyMs;
                hasLatencyData = true;
            }
            if (decodeLatencyMs > 0) {
                totalLatencyMs += decodeLatencyMs;
                hasLatencyData = true;
            }
            if (!hasLatencyData)
                totalLatencyMs = -1;
            // Packet loss calculation
            const packetsReceived = stats.transport.webrtcPacketsReceived ? parseInt(stats.transport.webrtcPacketsReceived) : 0;
            const packetsLost = stats.transport.webrtcPacketsLost ? parseInt(stats.transport.webrtcPacketsLost) : 0;
            const packetLossPercent = packetsReceived > 0 ? (packetsLost / (packetsReceived + packetsLost)) * 100 : -1;
            // Jitter: webrtcJitterSec is in seconds, convert to ms
            const jitterMs = stats.transport.webrtcJitterSec ? parseFloat(stats.transport.webrtcJitterSec) * 1000 : -1;
            // Resolution string
            const resolution = stats.videoWidth && stats.videoHeight
                ? `${stats.videoWidth}x${stats.videoHeight}`
                : "unknown";
            // Bitrate (not directly available, use -1)
            const bitrateMbps = -1;
            // Helper functions for individual metric quality
            const getRttQuality = (ms) => {
                if (ms <= 0)
                    return "good"; // No data = assume good
                if (ms < 50)
                    return "good";
                if (ms < 100)
                    return "warn";
                return "bad";
            };
            const getLatencyQuality = (ms) => {
                if (ms <= 0)
                    return "good"; // No data = assume good
                if (ms < 20)
                    return "good";
                if (ms < 50)
                    return "warn";
                return "bad";
            };
            const getFpsQuality = (current, target) => {
                if (current <= 0)
                    return "good"; // No data = assume good
                const diff = target - current;
                if (diff <= 5)
                    return "good";
                if (diff <= 15)
                    return "warn";
                return "bad";
            };
            const getPacketLossQuality = (percent) => {
                if (percent <= 0)
                    return "good";
                if (percent < 0.5)
                    return "good";
                if (percent < 2)
                    return "warn";
                return "bad";
            };
            const getJitterQuality = (ms) => {
                if (ms <= 0)
                    return "good"; // No data = assume good
                if (ms < 10)
                    return "good";
                if (ms < 30)
                    return "warn";
                return "bad";
            };
            // Calculate individual qualities
            const rttQuality = getRttQuality(networkRttMs);
            const hostLatQuality = getLatencyQuality(hostLatencyMs);
            const streamerLatQuality = getLatencyQuality(streamerLatencyMs);
            const decodeLatQuality = getLatencyQuality(decodeLatencyMs);
            const fpsQuality = getFpsQuality(currentFps, targetFps);
            const lossQuality = getPacketLossQuality(packetLossPercent);
            const jitterQuality = getJitterQuality(jitterMs);
            // Weighted scoring: higher weight = more important to stream quality
            const weights = [
                { quality: lossQuality, weight: 3 }, // Packet loss is critical
                { quality: fpsQuality, weight: 2 }, // FPS drops are noticeable
                { quality: rttQuality, weight: 2 }, // RTT affects responsiveness
                { quality: jitterQuality, weight: 2 }, // Jitter causes stuttering
                { quality: hostLatQuality, weight: 1 }, // Host latency informational
                { quality: streamerLatQuality, weight: 1 }, // Streamer latency informational
                { quality: decodeLatQuality, weight: 1 }, // Decode latency informational
            ];
            let totalScore = 0;
            let totalWeight = 0;
            for (const { quality: q, weight } of weights) {
                const score = q === "good" ? 0 : q === "warn" ? 1 : 2;
                totalScore += score * weight;
                totalWeight += weight;
            }
            const normalizedScore = totalScore / totalWeight;
            // Map to quality: <0.5 = good, <1.2 = fair, >=1.2 = poor
            let quality;
            if (normalizedScore < 0.5) {
                quality = "good";
            }
            else if (normalizedScore < 1.2) {
                quality = "fair";
            }
            else {
                quality = "poor";
            }
            return {
                quality,
                totalLatencyMs: Math.round(totalLatencyMs * 100) / 100,
                hostLatencyMs: Math.round(hostLatencyMs * 100) / 100,
                networkLatencyMs: Math.round(networkLatencyMs * 100) / 100,
                streamerLatencyMs: Math.round(streamerLatencyMs * 100) / 100,
                decodeLatencyMs: Math.round(decodeLatencyMs * 100) / 100,
                networkRttMs: Math.round(networkRttMs * 100) / 100,
                packetLossPercent: Math.round(packetLossPercent * 1000) / 1000,
                jitterMs: Math.round(jitterMs * 100) / 100,
                fps: Math.round(currentFps * 10) / 10,
                bitrateMbps,
                resolution,
                connectionType: connectionInfo.connectionType,
                isRelayConnection: connectionInfo.isRelay
            };
        });
    }
}
function createPrettyList(list) {
    let isFirst = true;
    let text = "[";
    for (const item of list) {
        if (!isFirst) {
            text += ", ";
        }
        isFirst = false;
        text += item;
    }
    text += "]";
    return text;
}
