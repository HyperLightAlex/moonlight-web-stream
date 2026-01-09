import { TrackAudioPlayer } from "./index.js";
export class AudioElementPlayer extends TrackAudioPlayer {
    static isBrowserSupported() {
        return "HTMLAudioElement" in window && "srcObject" in HTMLAudioElement.prototype;
    }
    constructor() {
        super("audio_element");
        this.audioElement = document.createElement("audio");
        this.oldTrack = null;
        this.stream = new MediaStream();
        this.audioElement.classList.add("audio-stream");
        this.audioElement.preload = "none";
        this.audioElement.controls = false;
        this.audioElement.autoplay = true;
        this.audioElement.muted = true;
        this.audioElement.srcObject = this.stream;
    }
    setup(_setup) {
        return true;
    }
    cleanup() {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack);
            this.oldTrack = null;
        }
        this.audioElement.srcObject = null;
    }
    setTrack(track) {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack);
            this.oldTrack = null;
        }
        this.stream.addTrack(track);
        this.oldTrack = track;
    }
    onUserInteraction() {
        console.info("[AudioElement]: onUserInteraction - unmuting and playing audio");
        this.audioElement.muted = false;
        // Explicitly call play() in case autoplay didn't work
        this.audioElement.play().then(() => {
            console.info("[AudioElement]: Audio playback started successfully");
        }).catch(error => {
            console.warn(`[AudioElement]: Audio play failed: ${error.message || error}`);
        });
    }
    mount(parent) {
        parent.appendChild(this.audioElement);
    }
    unmount(parent) {
        parent.removeChild(this.audioElement);
    }
}
