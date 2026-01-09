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
import { ByteBuffer } from "./buffer.js";
function num(value, decimals = 1) {
    if (value == null)
        return "-";
    return value.toFixed(decimals);
}
function getLatencyQuality(ms) {
    if (ms == null)
        return "good";
    if (ms < 20)
        return "good";
    if (ms < 50)
        return "warn";
    return "bad";
}
function getRttQuality(ms) {
    if (ms == null)
        return "good";
    if (ms < 50)
        return "good";
    if (ms < 100)
        return "warn";
    return "bad";
}
function getFpsQuality(current, target) {
    if (current == null || target == null)
        return "good";
    const diff = target - current;
    if (diff <= 5)
        return "good"; // Up to 5fps drop is fine
    if (diff <= 15)
        return "warn"; // 6-15fps drop is noticeable
    return "bad"; // >15fps drop is significant
}
function getPacketLossQuality(lost, received) {
    if (received === 0)
        return "good";
    const percent = (lost / (lost + received)) * 100;
    if (percent < 0.5)
        return "good";
    if (percent < 2)
        return "warn";
    return "bad";
}
function qualityClass(level) {
    return `stats-${level}`;
}
function formatMs(value, decimals = 1) {
    if (value == null)
        return "-";
    return `${value.toFixed(decimals)}ms`;
}
export function streamStatsToText(statsData) {
    // Legacy text format for backwards compatibility
    let text = `stats:
video: ${statsData.videoCodec}${statsData.decoderImplementation ? ` (${statsData.decoderImplementation})` : ""}, ${statsData.videoWidth}x${statsData.videoHeight}, ${statsData.videoFps} fps
rtt: ${formatMs(statsData.streamerRttMs)} (var: ${formatMs(statsData.streamerRttVarianceMs)})
host latency: ${formatMs(statsData.avgHostProcessingLatencyMs)}
streamer latency: ${formatMs(statsData.avgStreamerProcessingTimeMs)}
`;
    const webrtcFps = statsData.transport.webrtcFps;
    const packetsLost = statsData.transport.webrtcPacketsLost;
    const packetsReceived = statsData.transport.webrtcPacketsReceived;
    if (webrtcFps)
        text += `decode fps: ${webrtcFps}\n`;
    if (packetsLost && packetsReceived) {
        const lost = parseInt(packetsLost);
        const received = parseInt(packetsReceived);
        const percent = received > 0 ? ((lost / (lost + received)) * 100).toFixed(2) : "0";
        text += `packet loss: ${percent}% (${lost}/${lost + received})\n`;
    }
    return text;
}
export function streamStatsToHtml(statsData) {
    // Get RTT: prefer WebRTC RTT (already in ms), fallback to streamer RTT
    const webrtcRttMs = statsData.transport.webrtcRttMs ? parseFloat(statsData.transport.webrtcRttMs) : null;
    const rttMs = webrtcRttMs !== null && webrtcRttMs !== void 0 ? webrtcRttMs : statsData.streamerRttMs;
    const rttQuality = getRttQuality(rttMs);
    const hostLatencyQuality = getLatencyQuality(statsData.avgHostProcessingLatencyMs);
    const streamerLatencyQuality = getLatencyQuality(statsData.avgStreamerProcessingTimeMs);
    // Decode latency from WebRTC (already in ms)
    const decodeLatencyMs = statsData.transport.webrtcAvgDecodeTimeMs ? parseFloat(statsData.transport.webrtcAvgDecodeTimeMs) : null;
    const decodeLatencyQuality = getLatencyQuality(decodeLatencyMs);
    const webrtcFps = statsData.transport.webrtcFps ? parseFloat(statsData.transport.webrtcFps) : null;
    const fpsQuality = getFpsQuality(webrtcFps, statsData.videoFps);
    const packetsLost = statsData.transport.webrtcPacketsLost ? parseInt(statsData.transport.webrtcPacketsLost) : 0;
    const packetsReceived = statsData.transport.webrtcPacketsReceived ? parseInt(statsData.transport.webrtcPacketsReceived) : 0;
    const packetLossQuality = getPacketLossQuality(packetsLost, packetsReceived);
    const packetLossPercent = packetsReceived > 0 ? ((packetsLost / (packetsLost + packetsReceived)) * 100) : 0;
    // webrtcJitterSec is in seconds, convert to ms
    const jitterMs = statsData.transport.webrtcJitterSec ? parseFloat(statsData.transport.webrtcJitterSec) * 1000 : null;
    const jitterQuality = jitterMs != null && jitterMs > 30 ? "bad" : jitterMs != null && jitterMs > 10 ? "warn" : "good";
    // Calculate overall quality using weighted scoring
    // Weights: higher = more important to stream quality
    const weights = [
        { quality: packetLossQuality, weight: 3 }, // Packet loss is critical - causes artifacts
        { quality: fpsQuality, weight: 2 }, // FPS drops are very noticeable
        { quality: rttQuality, weight: 2 }, // RTT affects input responsiveness
        { quality: jitterQuality, weight: 2 }, // Jitter causes stuttering, buffer bloat
        { quality: hostLatencyQuality, weight: 1 }, // Host latency is informational
        { quality: streamerLatencyQuality, weight: 1 }, // Streamer latency is informational
    ];
    // Calculate weighted score: good=0, warn=1, bad=2
    let totalScore = 0;
    let totalWeight = 0;
    for (const { quality, weight } of weights) {
        const score = quality === "good" ? 0 : quality === "warn" ? 1 : 2;
        totalScore += score * weight;
        totalWeight += weight;
    }
    // Normalize to 0-2 range
    const normalizedScore = totalScore / totalWeight;
    // Map to quality: <0.5 = good, <1.2 = fair, >=1.2 = poor
    let overallQuality;
    if (normalizedScore < 0.5) {
        overallQuality = "good";
    }
    else if (normalizedScore < 1.2) {
        overallQuality = "warn";
    }
    else {
        overallQuality = "bad";
    }
    const overallLabel = overallQuality === "good" ? "Good" : overallQuality === "warn" ? "Fair" : "Poor";
    // Collect issues when quality is not good
    const issues = [];
    if (rttQuality !== "good" && rttMs != null) {
        issues.push({
            metric: "Network RTT",
            value: formatMs(rttMs),
            severity: rttQuality,
            suggestion: rttQuality === "bad"
                ? "High latency - check network connection or try wired ethernet"
                : "Moderate latency - streaming over WiFi or internet?"
        });
    }
    if (decodeLatencyQuality !== "good" && decodeLatencyMs != null) {
        issues.push({
            metric: "Decode Time",
            value: formatMs(decodeLatencyMs),
            severity: decodeLatencyQuality,
            suggestion: decodeLatencyQuality === "bad"
                ? "Slow decoding - device may be underpowered or codec unsupported"
                : "Decode time elevated - close other apps using hardware decoder"
        });
    }
    if (hostLatencyQuality !== "good" && statsData.avgHostProcessingLatencyMs != null) {
        issues.push({
            metric: "Host Encode",
            value: formatMs(statsData.avgHostProcessingLatencyMs),
            severity: hostLatencyQuality,
            suggestion: hostLatencyQuality === "bad"
                ? "Host PC struggling to encode - lower resolution or check GPU load"
                : "Host encode time elevated - consider closing other GPU apps"
        });
    }
    if (streamerLatencyQuality !== "good" && statsData.avgStreamerProcessingTimeMs != null) {
        issues.push({
            metric: "Streamer",
            value: formatMs(statsData.avgStreamerProcessingTimeMs),
            severity: streamerLatencyQuality,
            suggestion: "Streamer processing delayed - server may be under load"
        });
    }
    if (fpsQuality !== "good" && webrtcFps != null && statsData.videoFps != null) {
        const fpsDrop = statsData.videoFps - webrtcFps;
        issues.push({
            metric: "FPS Drop",
            value: `${fpsDrop.toFixed(0)} fps below target`,
            severity: fpsQuality,
            suggestion: fpsQuality === "bad"
                ? "Significant frame drops - network congestion or decoder overload"
                : "Minor frame drops - may improve with better connection"
        });
    }
    if (packetLossQuality !== "good") {
        issues.push({
            metric: "Packet Loss",
            value: `${packetLossPercent.toFixed(2)}%`,
            severity: packetLossQuality,
            suggestion: packetLossQuality === "bad"
                ? "High packet loss - unstable network, try wired connection"
                : "Some packet loss - WiFi interference or network congestion"
        });
    }
    if (jitterQuality !== "good" && jitterMs != null) {
        issues.push({
            metric: "Jitter",
            value: formatMs(jitterMs),
            severity: jitterQuality,
            suggestion: "Network timing inconsistent - other devices using bandwidth?"
        });
    }
    // Sort issues by severity (bad first, then warn) and limit to top 3
    issues.sort((a, b) => {
        if (a.severity === "bad" && b.severity !== "bad")
            return -1;
        if (a.severity !== "bad" && b.severity === "bad")
            return 1;
        return 0;
    });
    const topIssues = issues.slice(0, 3);
    // Build compact issues HTML
    let issuesHtml = "";
    if (topIssues.length > 0) {
        issuesHtml = `
    <div class="stats-section stats-issues">
        <div class="stats-section-title">⚠️ Issues</div>
        ${topIssues.map(issue => `
        <div class="stats-row stats-issue-row ${qualityClass(issue.severity)}">
            <span class="stats-label">${issue.metric}</span>
            <span class="stats-value">${issue.value}</span>
        </div>`).join("")}
    </div>`;
    }
    // Check if we have any latency data at all
    const hasLatencyData = rttMs != null ||
        statsData.avgHostProcessingLatencyMs != null ||
        statsData.avgStreamerProcessingTimeMs != null ||
        decodeLatencyMs != null;
    // Build latency section only if we have data
    const latencySection = hasLatencyData ? `
    <div class="stats-section">
        <div class="stats-section-title">⏱️ Latency</div>
        ${rttMs != null ? `<div class="stats-row">
            <span class="stats-label">RTT</span>
            <span class="stats-value ${qualityClass(rttQuality)}">${formatMs(rttMs)}</span>
        </div>` : ""}
        ${statsData.avgHostProcessingLatencyMs != null ? `<div class="stats-row">
            <span class="stats-label">Encode</span>
            <span class="stats-value ${qualityClass(hostLatencyQuality)}">${formatMs(statsData.avgHostProcessingLatencyMs)}</span>
        </div>` : ""}
        ${statsData.avgStreamerProcessingTimeMs != null ? `<div class="stats-row">
            <span class="stats-label">Streamer</span>
            <span class="stats-value ${qualityClass(streamerLatencyQuality)}">${formatMs(statsData.avgStreamerProcessingTimeMs)}</span>
        </div>` : ""}
        ${decodeLatencyMs != null ? `<div class="stats-row">
            <span class="stats-label">Decode</span>
            <span class="stats-value ${qualityClass(decodeLatencyQuality)}">${formatMs(decodeLatencyMs)}</span>
        </div>` : ""}
    </div>` : "";
    return `
<div class="stats-panel">
    <div class="stats-header">
        <span class="stats-title">Stats</span>
        <span class="stats-quality ${qualityClass(overallQuality)}">${overallLabel}</span>
    </div>
    <div class="stats-section">
        <div class="stats-row">
            <span class="stats-label">Video</span>
            <span class="stats-value">${statsData.videoCodec || "?"}${statsData.decoderImplementation ? ` <span class="stats-dim">${statsData.decoderImplementation}</span>` : ""}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Resolution</span>
            <span class="stats-value">${statsData.videoWidth || "?"}×${statsData.videoHeight || "?"}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">FPS</span>
            <span class="stats-value ${qualityClass(fpsQuality)}">${num(webrtcFps, 0)}<span class="stats-dim">/${statsData.videoFps || "?"}</span></span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Loss</span>
            <span class="stats-value ${qualityClass(packetLossQuality)}">${packetLossPercent.toFixed(1)}%</span>
        </div>
        ${jitterMs != null ? `<div class="stats-row">
            <span class="stats-label">Jitter</span>
            <span class="stats-value ${qualityClass(jitterQuality)}">${formatMs(jitterMs, 0)}</span>
        </div>` : ""}
    </div>
${latencySection}
${issuesHtml}
</div>`;
}
export class StreamStats {
    constructor(logger) {
        this.logger = null;
        this.enabled = false; // Controls overlay visibility
        this.collecting = false; // Controls data collection (always on when transport set)
        this.transport = null;
        this.statsChannel = null;
        this.updateIntervalId = null;
        this.statsData = {
            videoCodec: null,
            decoderImplementation: null,
            videoWidth: null,
            videoHeight: null,
            videoFps: null,
            streamerRttMs: null,
            streamerRttVarianceMs: null,
            minHostProcessingLatencyMs: null,
            maxHostProcessingLatencyMs: null,
            avgHostProcessingLatencyMs: null,
            minStreamerProcessingTimeMs: null,
            maxStreamerProcessingTimeMs: null,
            avgStreamerProcessingTimeMs: null,
            transport: {}
        };
        this.buffer = new ByteBuffer(10000);
        if (logger) {
            this.logger = logger;
        }
    }
    setTransport(transport) {
        this.transport = transport;
        // Always start collecting when transport is set (for getStreamHealth API)
        this.startCollecting();
    }
    // Start collecting stats (separate from display)
    startCollecting() {
        var _a;
        if (this.collecting)
            return;
        this.collecting = true;
        // Set up stats channel for server-side latency data
        if (!this.statsChannel && this.transport) {
            const channel = this.transport.getChannel(TransportChannelId.STATS);
            if (channel.type != "data") {
                (_a = this.logger) === null || _a === void 0 ? void 0 : _a.debug(`Failed initialize debug transport channel because type is "${channel.type}" and not "data"`);
            }
            else {
                channel.addReceiveListener(this.onRawData.bind(this));
                this.statsChannel = channel;
            }
        }
        // Start interval for WebRTC transport stats
        if (this.updateIntervalId == null) {
            this.updateIntervalId = setInterval(this.updateLocalStats.bind(this), 1000);
        }
    }
    setEnabled(enabled) {
        this.enabled = enabled;
        // Note: collection continues regardless of enabled state
    }
    isEnabled() {
        return this.enabled;
    }
    toggle() {
        this.setEnabled(!this.isEnabled());
    }
    onRawData(data) {
        this.buffer.reset();
        this.buffer.putU8Array(new Uint8Array(data));
        this.buffer.flip();
        const textLength = this.buffer.getU16();
        const text = this.buffer.getUtf8Raw(textLength);
        const json = JSON.parse(text);
        this.onMessage(json);
    }
    onMessage(msg) {
        if ("Rtt" in msg) {
            this.statsData.streamerRttMs = msg.Rtt.rtt_ms;
            this.statsData.streamerRttVarianceMs = msg.Rtt.rtt_variance_ms;
        }
        else if ("Video" in msg) {
            if (msg.Video.host_processing_latency) {
                this.statsData.minHostProcessingLatencyMs = msg.Video.host_processing_latency.min_host_processing_latency_ms;
                this.statsData.maxHostProcessingLatencyMs = msg.Video.host_processing_latency.max_host_processing_latency_ms;
                this.statsData.avgHostProcessingLatencyMs = msg.Video.host_processing_latency.avg_host_processing_latency_ms;
            }
            else {
                this.statsData.minHostProcessingLatencyMs = null;
                this.statsData.maxHostProcessingLatencyMs = null;
                this.statsData.avgHostProcessingLatencyMs = null;
            }
            this.statsData.minStreamerProcessingTimeMs = msg.Video.min_streamer_processing_time_ms;
            this.statsData.maxStreamerProcessingTimeMs = msg.Video.max_streamer_processing_time_ms;
            this.statsData.avgStreamerProcessingTimeMs = msg.Video.avg_streamer_processing_time_ms;
        }
    }
    updateLocalStats() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            if (!this.transport) {
                console.debug("Cannot query stats without transport");
                return;
            }
            const stats = yield ((_a = this.transport) === null || _a === void 0 ? void 0 : _a.getStats());
            for (const key in stats) {
                const value = stats[key];
                this.statsData.transport[key] = value;
            }
        });
    }
    setVideoInfo(codec, width, height, fps) {
        this.statsData.videoCodec = codec;
        this.statsData.videoWidth = width;
        this.statsData.videoHeight = height;
        this.statsData.videoFps = fps;
    }
    getCurrentStats() {
        const data = {};
        Object.assign(data, this.statsData);
        return data;
    }
}
