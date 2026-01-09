import { getStreamRectCorrected, TrackVideoRenderer } from "./index.js";
export class VideoElementRenderer extends TrackVideoRenderer {
    static isBrowserSupported() {
        return "HTMLVideoElement" in window && "srcObject" in HTMLVideoElement.prototype;
    }
    constructor() {
        super("video_element");
        this.videoElement = document.createElement("video");
        this.oldTrack = null;
        this.stream = new MediaStream();
        this.size = null;
        this.videoElement.classList.add("video-stream");
        this.videoElement.preload = "none";
        this.videoElement.controls = false;
        this.videoElement.autoplay = true;
        this.videoElement.disablePictureInPicture = true;
        this.videoElement.playsInline = true;
        this.videoElement.muted = true;
        if ("srcObject" in this.videoElement) {
            try {
                this.videoElement.srcObject = this.stream;
            }
            catch (err) {
                if (err.name !== "TypeError") {
                    throw err;
                }
                console.error(err);
                throw `video_element renderer not supported: ${err}`;
            }
        }
    }
    setup(setup) {
        this.size = [setup.width, setup.height];
    }
    cleanup() {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack);
        }
        this.videoElement.srcObject = null;
    }
    setTrack(track) {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack);
        }
        this.stream.addTrack(track);
        this.oldTrack = track;
        // Explicitly try to play - some browsers/WebViews need this even with autoplay
        console.info(`[VideoElement]: Track set, attempting to play video`);
        this.videoElement.play().then(() => {
            console.info(`[VideoElement]: Video playback started successfully`);
        }).catch(error => {
            console.warn(`[VideoElement]: Auto-play failed (may need user interaction): ${error.message || error}`);
        });
    }
    mount(parent) {
        parent.appendChild(this.videoElement);
    }
    unmount(parent) {
        parent.removeChild(this.videoElement);
    }
    onUserInteraction() {
        if (this.videoElement.paused) {
            this.videoElement.play().then(() => {
                // Playing
            }).catch(error => {
                console.error(`Failed to play videoElement: ${error.message || error}`);
            });
        }
    }
    getStreamRect() {
        if (!this.size) {
            return new DOMRect();
        }
        return getStreamRectCorrected(this.videoElement.getBoundingClientRect(), this.size);
    }
}
