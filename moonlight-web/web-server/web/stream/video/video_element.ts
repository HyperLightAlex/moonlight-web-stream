import { getStreamRectCorrected, TrackVideoRenderer, VideoRendererSetup } from "./index.js";

export class VideoElementRenderer extends TrackVideoRenderer {
    static isBrowserSupported(): boolean {
        return "HTMLVideoElement" in window && "srcObject" in HTMLVideoElement.prototype
    }

    private videoElement = document.createElement("video")
    private oldTrack: MediaStreamTrack | null = null
    private stream = new MediaStream()

    private size: [number, number] | null = null
    private isHybridMode = false

    constructor() {
        super("video_element")

        this.videoElement.classList.add("video-stream")
        this.videoElement.preload = "none"
        this.videoElement.controls = false
        this.videoElement.autoplay = true
        this.videoElement.disablePictureInPicture = true
        this.videoElement.playsInline = true
        this.videoElement.muted = true
        
        // Check for hybrid mode - will apply styles after mount
        this.isHybridMode = document.body.classList.contains("hybrid-mode")

        if ("srcObject" in this.videoElement) {
            try {
                this.videoElement.srcObject = this.stream
            } catch (err: any) {
                if (err.name !== "TypeError") {
                    throw err;
                }

                console.error(err)
                throw `video_element renderer not supported: ${err}`
            }
        }
    }
    
    private applyHybridModeStyles(): void {
        console.info("[VideoElement]: Applying hybrid mode fullscreen styles")
        
        // Get actual viewport dimensions - 100vh doesn't work correctly in Android WebView
        const viewportWidth = window.innerWidth
        const viewportHeight = window.innerHeight
        console.info(`[VideoElement]: Viewport dimensions: ${viewportWidth}x${viewportHeight}`)
        
        // First, ensure html and body allow full height
        document.documentElement.style.cssText = `width: ${viewportWidth}px !important; height: ${viewportHeight}px !important; margin: 0 !important; padding: 0 !important; overflow: hidden !important;`
        document.body.style.cssText = `width: ${viewportWidth}px !important; height: ${viewportHeight}px !important; margin: 0 !important; padding: 0 !important; overflow: hidden !important; background: black !important;`
        
        // Use explicit pixel values instead of viewport units (Android WebView bug workaround)
        const styleString = [
            "position: fixed",
            "top: 0px",
            "left: 0px", 
            "right: 0px",
            "bottom: 0px",
            `width: ${viewportWidth}px`,
            `height: ${viewportHeight}px`,
            "max-width: none",
            "max-height: none",
            `min-width: ${viewportWidth}px`,
            `min-height: ${viewportHeight}px`,
            "object-fit: contain",
            "z-index: 9999",
            "background-color: black",
            "transform: none",
            "margin: 0",
            "padding: 0",
            "border: none",
            "display: block"
        ].join(" !important; ") + " !important;"
        
        this.videoElement.setAttribute("style", styleString)
        
        // Verify styles were applied
        const computed = window.getComputedStyle(this.videoElement)
        console.info(`[VideoElement]: After setAttribute - position: ${computed.position}, width: ${computed.width}, height: ${computed.height}`)
        console.info(`[VideoElement]: Video element offset size: ${this.videoElement.offsetWidth}x${this.videoElement.offsetHeight}`)
        
        // Handle orientation changes / resizes
        const resizeHandler = () => {
            const w = window.innerWidth
            const h = window.innerHeight
            console.info(`[VideoElement]: Resize detected: ${w}x${h}`)
            this.videoElement.style.width = `${w}px`
            this.videoElement.style.height = `${h}px`
            this.videoElement.style.minWidth = `${w}px`
            this.videoElement.style.minHeight = `${h}px`
        }
        window.addEventListener("resize", resizeHandler)
        window.addEventListener("orientationchange", resizeHandler)
    }

    setup(setup: VideoRendererSetup): void {
        this.size = [setup.width, setup.height]
    }
    cleanup(): void {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack)
        }
        this.videoElement.srcObject = null
    }

    setTrack(track: MediaStreamTrack): void {
        if (this.oldTrack) {
            this.stream.removeTrack(this.oldTrack)
        }

        this.stream.addTrack(track)
        this.oldTrack = track

        // Explicitly try to play - some browsers/WebViews need this even with autoplay
        console.info(`[VideoElement]: Track set, attempting to play video`)
        console.info(`[VideoElement]: Track state: enabled=${track.enabled}, muted=${track.muted}, readyState=${track.readyState}`)
        console.info(`[VideoElement]: Video element state: readyState=${this.videoElement.readyState}, paused=${this.videoElement.paused}`)
        console.info(`[VideoElement]: Video element size: ${this.videoElement.offsetWidth}x${this.videoElement.offsetHeight}`)
        
        this.videoElement.play().then(() => {
            console.info(`[VideoElement]: Video playback started successfully`)
            console.info(`[VideoElement]: After play - videoWidth=${this.videoElement.videoWidth}, videoHeight=${this.videoElement.videoHeight}`)
            console.info(`[VideoElement]: After play - offsetWidth=${this.videoElement.offsetWidth}, offsetHeight=${this.videoElement.offsetHeight}`)
            console.info(`[VideoElement]: After play - readyState=${this.videoElement.readyState}`)
            
            // Log when we get actual video frames
            this.videoElement.addEventListener('loadeddata', () => {
                console.info(`[VideoElement]: loadeddata - video dimensions: ${this.videoElement.videoWidth}x${this.videoElement.videoHeight}`)
            }, { once: true })
            
            this.videoElement.addEventListener('loadedmetadata', () => {
                console.info(`[VideoElement]: loadedmetadata - video dimensions: ${this.videoElement.videoWidth}x${this.videoElement.videoHeight}`)
            }, { once: true })
            
            // Check after a short delay to see if frames arrived
            setTimeout(() => {
                console.info(`[VideoElement]: Delayed check - videoWidth=${this.videoElement.videoWidth}, videoHeight=${this.videoElement.videoHeight}, readyState=${this.videoElement.readyState}`)
            }, 500)
        }).catch(error => {
            console.warn(`[VideoElement]: Auto-play failed (may need user interaction): ${error.message || error}`)
        })
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.videoElement)
        
        // Apply hybrid mode styles AFTER element is in DOM
        if (this.isHybridMode) {
            // Use requestAnimationFrame to ensure the element is fully in the DOM
            requestAnimationFrame(() => {
                this.applyHybridModeStyles()
            })
        }
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.videoElement)
    }

    onUserInteraction(): void {
        if (this.videoElement.paused) {
            this.videoElement.play().then(() => {
                // Playing
            }).catch(error => {
                console.error(`Failed to play videoElement: ${error.message || error}`);
            })
        }
    }
    getStreamRect(): DOMRect {
        if (!this.size) {
            return new DOMRect()
        }

        return getStreamRectCorrected(this.videoElement.getBoundingClientRect(), this.size)
    }
}