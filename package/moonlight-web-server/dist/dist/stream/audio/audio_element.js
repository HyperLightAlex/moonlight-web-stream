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
        this.audioElement.muted = false;
    }
    mount(parent) {
        parent.appendChild(this.audioElement);
    }
    unmount(parent) {
        parent.removeChild(this.audioElement);
    }
}
