import { StreamerStatsUpdate, TransportChannelId } from "../api_bindings.js"
import { ByteBuffer } from "./buffer.js"
import { Logger } from "./log.js"
import { DataTransportChannel, Transport } from "./transport/index.js"

export type StreamStatsData = {
    videoCodec: string | null
    decoderImplementation: string | null
    videoWidth: number | null
    videoHeight: number | null
    videoFps: number | null
    streamerRttMs: number | null
    streamerRttVarianceMs: number | null
    minHostProcessingLatencyMs: number | null
    maxHostProcessingLatencyMs: number | null
    avgHostProcessingLatencyMs: number | null
    minStreamerProcessingTimeMs: number | null
    maxStreamerProcessingTimeMs: number | null
    avgStreamerProcessingTimeMs: number | null
    transport: Record<string, string>
}

function num(value: number | null | undefined, decimals: number = 1): string {
    if (value == null) return "-"
    return value.toFixed(decimals)
}

// Color thresholds for different metrics
type QualityLevel = "good" | "warn" | "bad"

function getLatencyQuality(ms: number | null): QualityLevel {
    if (ms == null) return "good"
    if (ms < 20) return "good"
    if (ms < 50) return "warn"
    return "bad"
}

function getRttQuality(ms: number | null): QualityLevel {
    if (ms == null) return "good"
    if (ms < 50) return "good"
    if (ms < 100) return "warn"
    return "bad"
}

function getFpsQuality(current: number | null, target: number | null): QualityLevel {
    if (current == null || target == null) return "good"
    const diff = target - current
    if (diff <= 2) return "good"
    if (diff <= 10) return "warn"
    return "bad"
}

function getPacketLossQuality(lost: number, received: number): QualityLevel {
    if (received === 0) return "good"
    const percent = (lost / (lost + received)) * 100
    if (percent < 0.5) return "good"
    if (percent < 2) return "warn"
    return "bad"
}

function qualityClass(level: QualityLevel): string {
    return `stats-${level}`
}

function formatMs(value: number | null | undefined, decimals: number = 1): string {
    if (value == null) return "-"
    return `${value.toFixed(decimals)}ms`
}

export function streamStatsToText(statsData: StreamStatsData): string {
    // Legacy text format for backwards compatibility
    let text = `stats:
video: ${statsData.videoCodec}${statsData.decoderImplementation ? ` (${statsData.decoderImplementation})` : ""}, ${statsData.videoWidth}x${statsData.videoHeight}, ${statsData.videoFps} fps
rtt: ${formatMs(statsData.streamerRttMs)} (var: ${formatMs(statsData.streamerRttVarianceMs)})
host latency: ${formatMs(statsData.avgHostProcessingLatencyMs)}
streamer latency: ${formatMs(statsData.avgStreamerProcessingTimeMs)}
`
    const webrtcFps = statsData.transport.webrtcFps
    const packetsLost = statsData.transport.webrtcPacketsLost
    const packetsReceived = statsData.transport.webrtcPacketsReceived
    
    if (webrtcFps) text += `decode fps: ${webrtcFps}\n`
    if (packetsLost && packetsReceived) {
        const lost = parseInt(packetsLost)
        const received = parseInt(packetsReceived)
        const percent = received > 0 ? ((lost / (lost + received)) * 100).toFixed(2) : "0"
        text += `packet loss: ${percent}% (${lost}/${lost + received})\n`
    }

    return text
}

export function streamStatsToHtml(statsData: StreamStatsData): string {
    const rttQuality = getRttQuality(statsData.streamerRttMs)
    const hostLatencyQuality = getLatencyQuality(statsData.avgHostProcessingLatencyMs)
    const streamerLatencyQuality = getLatencyQuality(statsData.avgStreamerProcessingTimeMs)
    
    const webrtcFps = statsData.transport.webrtcFps ? parseFloat(statsData.transport.webrtcFps) : null
    const fpsQuality = getFpsQuality(webrtcFps, statsData.videoFps)
    
    const packetsLost = statsData.transport.webrtcPacketsLost ? parseInt(statsData.transport.webrtcPacketsLost) : 0
    const packetsReceived = statsData.transport.webrtcPacketsReceived ? parseInt(statsData.transport.webrtcPacketsReceived) : 0
    const packetLossQuality = getPacketLossQuality(packetsLost, packetsReceived)
    const packetLossPercent = packetsReceived > 0 ? ((packetsLost / (packetsLost + packetsReceived)) * 100) : 0
    
    const jitterMs = statsData.transport.webrtcJitterMs ? parseFloat(statsData.transport.webrtcJitterMs) * 1000 : null
    const jitterQuality = jitterMs != null && jitterMs > 10 ? "warn" : "good"
    
    // Calculate overall quality
    const qualities = [rttQuality, hostLatencyQuality, streamerLatencyQuality, fpsQuality, packetLossQuality]
    const hasBad = qualities.indexOf("bad") !== -1
    const hasWarn = qualities.indexOf("warn") !== -1
    const overallQuality: QualityLevel = hasBad ? "bad" : hasWarn ? "warn" : "good"
    const overallLabel = overallQuality === "good" ? "Good" : overallQuality === "warn" ? "Fair" : "Poor"
    
    return `
<div class="stats-panel">
    <div class="stats-header">
        <span class="stats-title">Stream Stats</span>
        <span class="stats-quality ${qualityClass(overallQuality)}">${overallLabel}</span>
    </div>
    
    <div class="stats-section">
        <div class="stats-section-title">📺 Video</div>
        <div class="stats-row">
            <span class="stats-label">Codec</span>
            <span class="stats-value">${statsData.videoCodec || "-"}${statsData.decoderImplementation ? ` <span class="stats-dim">(${statsData.decoderImplementation})</span>` : ""}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Resolution</span>
            <span class="stats-value">${statsData.videoWidth || "-"}×${statsData.videoHeight || "-"}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">FPS</span>
            <span class="stats-value ${qualityClass(fpsQuality)}">${num(webrtcFps, 0)} <span class="stats-dim">/ ${statsData.videoFps || "-"}</span></span>
        </div>
    </div>
    
    <div class="stats-section">
        <div class="stats-section-title">⏱️ Latency</div>
        <div class="stats-row">
            <span class="stats-label">Network RTT</span>
            <span class="stats-value ${qualityClass(rttQuality)}">${formatMs(statsData.streamerRttMs)}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Host Encode</span>
            <span class="stats-value ${qualityClass(hostLatencyQuality)}">${formatMs(statsData.avgHostProcessingLatencyMs)}</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Streamer</span>
            <span class="stats-value ${qualityClass(streamerLatencyQuality)}">${formatMs(statsData.avgStreamerProcessingTimeMs)}</span>
        </div>
    </div>
    
    <div class="stats-section">
        <div class="stats-section-title">📡 Network</div>
        <div class="stats-row">
            <span class="stats-label">Packet Loss</span>
            <span class="stats-value ${qualityClass(packetLossQuality)}">${packetLossPercent.toFixed(2)}%</span>
        </div>
        <div class="stats-row">
            <span class="stats-label">Jitter</span>
            <span class="stats-value ${qualityClass(jitterQuality)}">${jitterMs != null ? formatMs(jitterMs) : "-"}</span>
        </div>
    </div>
</div>`
}

export class StreamStats {

    private logger: Logger | null = null

    private enabled: boolean = false
    private transport: Transport | null = null
    private statsChannel: DataTransportChannel | null = null
    private updateIntervalId: number | null = null

    private statsData: StreamStatsData = {
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
    }

    constructor(logger?: Logger) {
        if (logger) {
            this.logger = logger
        }
    }

    setTransport(transport: Transport) {
        this.transport = transport

        this.checkEnabled()
    }
    private checkEnabled() {
        if (this.enabled) {
            if (this.statsChannel) {
                this.statsChannel.removeReceiveListener(this.onRawData.bind(this))
                this.statsChannel = null
            }

            if (!this.statsChannel && this.transport) {
                const channel = this.transport.getChannel(TransportChannelId.STATS)
                if (channel.type != "data") {
                    this.logger?.debug(`Failed initialize debug transport channel because type is "${channel.type}" and not "data"`)
                    return
                }
                channel.addReceiveListener(this.onRawData.bind(this))
                this.statsChannel = channel
            }
            if (this.updateIntervalId == null) {
                this.updateIntervalId = setInterval(this.updateLocalStats.bind(this), 1000)
            }
        } else {
            if (this.updateIntervalId != null) {
                clearInterval(this.updateIntervalId)
                this.updateIntervalId = null
            }
        }
    }

    setEnabled(enabled: boolean) {
        this.enabled = enabled

        this.checkEnabled()
    }
    isEnabled(): boolean {
        return this.enabled
    }
    toggle() {
        this.setEnabled(!this.isEnabled())
    }

    private buffer: ByteBuffer = new ByteBuffer(10000)
    private onRawData(data: ArrayBuffer) {
        this.buffer.reset()
        this.buffer.putU8Array(new Uint8Array(data))

        this.buffer.flip()

        const textLength = this.buffer.getU16()
        const text = this.buffer.getUtf8Raw(textLength)

        const json: StreamerStatsUpdate = JSON.parse(text)
        this.onMessage(json)
    }
    private onMessage(msg: StreamerStatsUpdate) {
        if ("Rtt" in msg) {
            this.statsData.streamerRttMs = msg.Rtt.rtt_ms
            this.statsData.streamerRttVarianceMs = msg.Rtt.rtt_variance_ms
        } else if ("Video" in msg) {
            if (msg.Video.host_processing_latency) {
                this.statsData.minHostProcessingLatencyMs = msg.Video.host_processing_latency.min_host_processing_latency_ms
                this.statsData.maxHostProcessingLatencyMs = msg.Video.host_processing_latency.max_host_processing_latency_ms
                this.statsData.avgHostProcessingLatencyMs = msg.Video.host_processing_latency.avg_host_processing_latency_ms
            } else {
                this.statsData.minHostProcessingLatencyMs = null
                this.statsData.maxHostProcessingLatencyMs = null
                this.statsData.avgHostProcessingLatencyMs = null
            }

            this.statsData.minStreamerProcessingTimeMs = msg.Video.min_streamer_processing_time_ms
            this.statsData.maxStreamerProcessingTimeMs = msg.Video.max_streamer_processing_time_ms
            this.statsData.avgStreamerProcessingTimeMs = msg.Video.avg_streamer_processing_time_ms
        }
    }

    private async updateLocalStats() {
        if (!this.transport) {
            console.debug("Cannot query stats without transport")
            return
        }

        const stats = await this.transport?.getStats()
        for (const key in stats) {
            const value = stats[key]

            this.statsData.transport[key] = value
        }
    }

    setVideoInfo(codec: string, width: number, height: number, fps: number) {
        this.statsData.videoCodec = codec
        this.statsData.videoWidth = width
        this.statsData.videoHeight = height
        this.statsData.videoFps = fps
    }

    getCurrentStats(): StreamStatsData {
        const data = {}
        Object.assign(data, this.statsData)
        return data as StreamStatsData
    }
}