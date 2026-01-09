import { CanvasVideoRenderer } from "./canvas_element.js";
import { VideoElementRenderer } from "./video_element.js";
import { MediaStreamTrackProcessorPipe } from "./media_stream_track_processor_pipe.js";
const FINAL_VIDEO_RENDERER = [
    VideoElementRenderer,
    CanvasVideoRenderer
];
const PIPE_TYPES = ["videotrack", "videoframe"];
const VIDEO_PIPES = {
    videotrack_to_videoframe: MediaStreamTrackProcessorPipe
};
export function buildVideoPipeline(type, settings) {
    let log = `Building video pipeline with output "${type}"`;
    // Forced renderer
    if (settings.canvasRenderer) {
        if (type == "videotrack" && MediaStreamTrackProcessorPipe.isBrowserSupported() && CanvasVideoRenderer.isBrowserSupported()) {
            const videoRenderer = new MediaStreamTrackProcessorPipe(new CanvasVideoRenderer());
            return { videoRenderer, log, error: null };
        }
        else {
            throw "Failed to create video canvas renderer because it is not supported this this browser";
        }
    }
    // TODO dynamically create pipelines based on browser support
    const directVideoRenderers = FINAL_VIDEO_RENDERER.filter(entry => entry.type == type && entry.isBrowserSupported());
    if (directVideoRenderers.length >= 1) {
        const videoRenderer = new directVideoRenderers[0];
        return { videoRenderer, log, error: null };
    }
    return { videoRenderer: null, log, error: "No supported video renderer found!" };
}
