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
        // Force fullscreen styles directly on video element for hybrid mode
        // This ensures the video fills the viewport regardless of CSS loading issues
        // NOTE: Must use setProperty with 'important' flag - cssText doesn't handle !important correctly
        if (document.body.classList.contains("hybrid-mode")) {
            console.info("[VideoElement]: Applying hybrid mode inline styles via setProperty");
            const style = this.videoElement.style;
            style.setProperty("position", "fixed", "important");
            style.setProperty("top", "0", "important");
            style.setProperty("left", "0", "important");
            style.setProperty("width", "100vw", "important");
            style.setProperty("height", "100vh", "important");
            style.setProperty("max-width", "100vw", "important");
            style.setProperty("max-height", "100vh", "important");
            style.setProperty("min-width", "100vw", "important");
            style.setProperty("min-height", "100vh", "important");
            style.setProperty("object-fit", "contain", "important");
            style.setProperty("z-index", "9999", "important");
            style.setProperty("background-color", "black", "important");
            style.setProperty("transform", "none", "important");
            style.setProperty("margin", "0", "important");
            style.setProperty("padding", "0", "important");
            style.setProperty("border", "none", "important");
            style.setProperty("display", "block", "important");
            console.info(`[VideoElement]: Styles applied. Computed style check - position: ${window.getComputedStyle(this.videoElement).position}`);
        }
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
        console.info(`[VideoElement]: Track state: enabled=${track.enabled}, muted=${track.muted}, readyState=${track.readyState}`);
        console.info(`[VideoElement]: Video element state: readyState=${this.videoElement.readyState}, paused=${this.videoElement.paused}`);
        console.info(`[VideoElement]: Video element size: ${this.videoElement.offsetWidth}x${this.videoElement.offsetHeight}`);
        this.videoElement.play().then(() => {
            console.info(`[VideoElement]: Video playback started successfully`);
            console.info(`[VideoElement]: After play - videoWidth=${this.videoElement.videoWidth}, videoHeight=${this.videoElement.videoHeight}`);
            console.info(`[VideoElement]: After play - offsetWidth=${this.videoElement.offsetWidth}, offsetHeight=${this.videoElement.offsetHeight}`);
            console.info(`[VideoElement]: After play - readyState=${this.videoElement.readyState}`);
            // Log when we get actual video frames
            this.videoElement.addEventListener('loadeddata', () => {
                console.info(`[VideoElement]: loadeddata - video dimensions: ${this.videoElement.videoWidth}x${this.videoElement.videoHeight}`);
            }, { once: true });
            this.videoElement.addEventListener('loadedmetadata', () => {
                console.info(`[VideoElement]: loadedmetadata - video dimensions: ${this.videoElement.videoWidth}x${this.videoElement.videoHeight}`);
            }, { once: true });
            // Check after a short delay to see if frames arrived
            setTimeout(() => {
                console.info(`[VideoElement]: Delayed check - videoWidth=${this.videoElement.videoWidth}, videoHeight=${this.videoElement.videoHeight}, readyState=${this.videoElement.readyState}`);
            }, 500);
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
