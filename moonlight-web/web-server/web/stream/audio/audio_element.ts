import { AudioPlayerSetup, TrackAudioPlayer } from "./index.js";

export class AudioElementPlayer extends TrackAudioPlayer {

    static isBrowserSupported(): boolean {
        return "HTMLAudioElement" in window && "srcObject" in HTMLAudioElement.prototype
    }

    private audioElement = document.createElement("audio")
    private oldTrack: MediaStreamTrack | null = null
    private stream = new MediaStream()

    constructor() {
        super("audio_element")

        this.audioElement.classList.add("audio-stream")
        this.audioElement.preload = "none"
        this.audioElement.controls = false
        this.audioElement.autoplay = true
        this.audioElement.muted = true
        this.audioElement.srcObject = this.stream
    }

    setup(_setup: AudioPlayerSetup) {
        return true
    }
    cleanup(): void {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack)
            this.oldTrack = null
        }
        this.audioElement.srcObject = null
    }

    setTrack(track: MediaStreamTrack): void {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack)
            this.oldTrack = null
        }

        this.stream.addTrack(track)
        this.oldTrack = track
    }

    onUserInteraction(): void {
        console.info("[AudioElement]: onUserInteraction - unmuting and playing audio")
        this.audioElement.muted = false
        
        // Explicitly call play() in case autoplay didn't work
        this.audioElement.play().then(() => {
            console.info("[AudioElement]: Audio playback started successfully")
        }).catch(error => {
            console.warn(`[AudioElement]: Audio play failed: ${error.message || error}`)
        })
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.audioElement)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.audioElement)
    }
}