import { ComponentEvent } from "./index.js";
import { InputComponent, SelectComponent } from "./input.js";
export function defaultStreamSettings() {
    return {
        sidebarEdge: "left",
        bitrate: 10000,
        packetSize: 2048,
        fps: 60,
        videoFrameQueueSize: 3,
        videoSize: "custom",
        videoSizeCustom: {
            width: 1920,
            height: 1080,
        },
        videoCodec: "h264",
        videoForceCodec: false,
        canvasRenderer: false,
        playAudioLocal: false,
        audioSampleQueueSize: 20,
        mouseScrollMode: "highres",
        controllerConfig: {
            invertAB: false,
            invertXY: false,
            sendIntervalOverride: null,
        },
        dataTransport: "auto",
        toggleFullscreenWithKeybind: false
    };
}
export function getLocalStreamSettings() {
    let settings = null;
    try {
        const settingsLoadedJson = localStorage.getItem("mlSettings");
        if (settingsLoadedJson == null) {
            return null;
        }
        const settingsLoaded = JSON.parse(settingsLoadedJson);
        settings = defaultStreamSettings();
        Object.assign(settings, settingsLoaded);
    }
    catch (e) {
        localStorage.removeItem("mlSettings");
    }
    return settings;
}
export function setLocalStreamSettings(settings) {
    localStorage.setItem("mlSettings", JSON.stringify(settings));
}
export class StreamSettingsComponent {
    constructor(settings) {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k;
        this.divElement = document.createElement("div");
        this.sidebarHeader = document.createElement("h2");
        this.streamHeader = document.createElement("h2");
        this.audioHeader = document.createElement("h2");
        this.mouseHeader = document.createElement("h2");
        this.controllerHeader = document.createElement("h2");
        this.otherHeader = document.createElement("h2");
        const defaultSettings = defaultStreamSettings();
        // Root div
        this.divElement.classList.add("settings");
        // Sidebar
        this.sidebarHeader.innerText = "Sidebar";
        this.divElement.appendChild(this.sidebarHeader);
        this.sidebarEdge = new SelectComponent("sidebarEdge", [
            { value: "left", name: "Left" },
            { value: "right", name: "Right" },
            { value: "up", name: "Up" },
            { value: "down", name: "Down" },
        ], {
            displayName: "Sidebar Edge",
            preSelectedOption: (_a = settings === null || settings === void 0 ? void 0 : settings.sidebarEdge) !== null && _a !== void 0 ? _a : defaultSettings.sidebarEdge,
        });
        this.sidebarEdge.addChangeListener(this.onSettingsChange.bind(this));
        this.sidebarEdge.mount(this.divElement);
        // Video
        this.streamHeader.innerText = "Video";
        this.divElement.appendChild(this.streamHeader);
        // Bitrate
        this.bitrate = new InputComponent("bitrate", "number", "Bitrate", {
            defaultValue: defaultSettings.bitrate.toString(),
            value: (_b = settings === null || settings === void 0 ? void 0 : settings.bitrate) === null || _b === void 0 ? void 0 : _b.toString(),
            step: "100",
            numberSlider: {
                // TODO: values?
                range_min: 1000,
                range_max: 10000,
            }
        });
        this.bitrate.addChangeListener(this.onSettingsChange.bind(this));
        this.bitrate.mount(this.divElement);
        // Packet Size
        this.packetSize = new InputComponent("packetSize", "number", "Packet Size", {
            defaultValue: defaultSettings.packetSize.toString(),
            value: (_c = settings === null || settings === void 0 ? void 0 : settings.packetSize) === null || _c === void 0 ? void 0 : _c.toString(),
            step: "100"
        });
        this.packetSize.addChangeListener(this.onSettingsChange.bind(this));
        this.packetSize.mount(this.divElement);
        // Fps
        this.fps = new InputComponent("fps", "number", "Fps", {
            defaultValue: defaultSettings.fps.toString(),
            value: (_d = settings === null || settings === void 0 ? void 0 : settings.fps) === null || _d === void 0 ? void 0 : _d.toString(),
            step: "100"
        });
        this.fps.addChangeListener(this.onSettingsChange.bind(this));
        this.fps.mount(this.divElement);
        // Video Size
        this.videoSize = new SelectComponent("videoSize", [
            { value: "720p", name: "720p" },
            { value: "1080p", name: "1080p" },
            { value: "1440p", name: "1440p" },
            { value: "4k", name: "4k" },
            { value: "native", name: "native" },
            { value: "custom", name: "custom" }
        ], {
            displayName: "Video Size",
            preSelectedOption: (settings === null || settings === void 0 ? void 0 : settings.videoSize) || defaultSettings.videoSize
        });
        this.videoSize.addChangeListener(this.onSettingsChange.bind(this));
        this.videoSize.mount(this.divElement);
        this.videoSizeWidth = new InputComponent("videoSizeWidth", "number", "Video Width", {
            defaultValue: defaultSettings.videoSizeCustom.width.toString(),
            value: settings === null || settings === void 0 ? void 0 : settings.videoSizeCustom.width.toString()
        });
        this.videoSizeWidth.addChangeListener(this.onSettingsChange.bind(this));
        this.videoSizeWidth.mount(this.divElement);
        this.videoSizeHeight = new InputComponent("videoSizeHeight", "number", "Video Height", {
            defaultValue: defaultSettings.videoSizeCustom.height.toString(),
            value: settings === null || settings === void 0 ? void 0 : settings.videoSizeCustom.height.toString()
        });
        this.videoSizeHeight.addChangeListener(this.onSettingsChange.bind(this));
        this.videoSizeHeight.mount(this.divElement);
        // Video Sample Queue Size
        this.videoSampleQueueSize = new InputComponent("videoFrameQueueSize", "number", "Video Frame Queue Size", {
            defaultValue: defaultSettings.videoFrameQueueSize.toString(),
            value: (_e = settings === null || settings === void 0 ? void 0 : settings.videoFrameQueueSize) === null || _e === void 0 ? void 0 : _e.toString()
        });
        this.videoSampleQueueSize.addChangeListener(this.onSettingsChange.bind(this));
        this.videoSampleQueueSize.mount(this.divElement);
        // Codec
        this.videoCodec = new SelectComponent("videoCodec", [
            { value: "h264", name: "H264 (Default)" },
            { value: "auto", name: "Auto (Experimental)" },
            { value: "h265", name: "H265" },
            { value: "av1", name: "AV1 (Experimental)" },
        ], {
            displayName: "Video Codec",
            preSelectedOption: (_f = settings === null || settings === void 0 ? void 0 : settings.videoCodec) !== null && _f !== void 0 ? _f : defaultSettings.videoCodec
        });
        this.videoCodec.addChangeListener(this.onSettingsChange.bind(this));
        this.videoCodec.mount(this.divElement);
        // Force Codec
        this.videoForceCodec = new InputComponent("videoForceCodec", "checkbox", "Force Video Codec", {
            checked: (_g = settings === null || settings === void 0 ? void 0 : settings.videoForceCodec) !== null && _g !== void 0 ? _g : defaultSettings.videoForceCodec
        });
        this.videoForceCodec.addChangeListener(this.onSettingsChange.bind(this));
        this.videoForceCodec.mount(this.divElement);
        // Use Canvas Renderer
        this.canvasRenderer = new InputComponent("canvasRenderer", "checkbox", "Use Canvas Renderer (Experimental)", {
            defaultValue: defaultSettings.canvasRenderer.toString(),
            checked: settings === null || settings === void 0 ? void 0 : settings.canvasRenderer
        });
        this.canvasRenderer.addChangeListener(this.onSettingsChange.bind(this));
        this.canvasRenderer.mount(this.divElement);
        // Audio local
        this.audioHeader.innerText = "Audio";
        this.divElement.appendChild(this.audioHeader);
        this.playAudioLocal = new InputComponent("playAudioLocal", "checkbox", "Play Audio Local", {
            checked: settings === null || settings === void 0 ? void 0 : settings.playAudioLocal
        });
        this.playAudioLocal.addChangeListener(this.onSettingsChange.bind(this));
        this.playAudioLocal.mount(this.divElement);
        // Audio Sample Queue Size
        this.audioSampleQueueSize = new InputComponent("audioSampleQueueSize", "number", "Audio Sample Queue Size", {
            defaultValue: defaultSettings.audioSampleQueueSize.toString(),
            value: (_h = settings === null || settings === void 0 ? void 0 : settings.audioSampleQueueSize) === null || _h === void 0 ? void 0 : _h.toString()
        });
        this.audioSampleQueueSize.addChangeListener(this.onSettingsChange.bind(this));
        this.audioSampleQueueSize.mount(this.divElement);
        // Mouse
        this.mouseHeader.innerText = "Mouse";
        this.divElement.appendChild(this.mouseHeader);
        this.mouseScrollMode = new SelectComponent("mouseScrollMode", [
            { value: "highres", name: "High Res" },
            { value: "normal", name: "Normal" }
        ], {
            displayName: "Scroll Mode",
            preSelectedOption: (settings === null || settings === void 0 ? void 0 : settings.mouseScrollMode) || defaultSettings.mouseScrollMode
        });
        this.mouseScrollMode.addChangeListener(this.onSettingsChange.bind(this));
        this.mouseScrollMode.mount(this.divElement);
        // Controller
        if (window.isSecureContext) {
            this.controllerHeader.innerText = "Controller";
        }
        else {
            this.controllerHeader.innerText = "Controller (Disabled: Secure Context Required)";
        }
        this.divElement.appendChild(this.controllerHeader);
        this.controllerInvertAB = new InputComponent("controllerInvertAB", "checkbox", "Invert A and B", {
            checked: settings === null || settings === void 0 ? void 0 : settings.controllerConfig.invertAB
        });
        this.controllerInvertAB.addChangeListener(this.onSettingsChange.bind(this));
        this.controllerInvertAB.mount(this.divElement);
        this.controllerInvertXY = new InputComponent("controllerInvertXY", "checkbox", "Invert X and Y", {
            checked: settings === null || settings === void 0 ? void 0 : settings.controllerConfig.invertXY
        });
        this.controllerInvertXY.addChangeListener(this.onSettingsChange.bind(this));
        this.controllerInvertXY.mount(this.divElement);
        // Controller Send Interval
        this.controllerSendIntervalOverride = new InputComponent("controllerSendIntervalOverride", "number", "Override Controller State Send Interval", {
            hasEnableCheckbox: true,
            defaultValue: "20",
            value: (_j = settings === null || settings === void 0 ? void 0 : settings.controllerConfig.sendIntervalOverride) === null || _j === void 0 ? void 0 : _j.toString(),
            numberSlider: {
                range_min: 10,
                range_max: 120
            }
        });
        this.controllerSendIntervalOverride.setEnabled((settings === null || settings === void 0 ? void 0 : settings.controllerConfig.sendIntervalOverride) != null);
        this.controllerSendIntervalOverride.addChangeListener(this.onSettingsChange.bind(this));
        this.controllerSendIntervalOverride.mount(this.divElement);
        if (!window.isSecureContext) {
            this.controllerInvertAB.setEnabled(false);
            this.controllerInvertXY.setEnabled(false);
        }
        // Other
        this.otherHeader.innerText = "Other";
        this.divElement.appendChild(this.otherHeader);
        this.dataTransport = new SelectComponent("transport", [
            { value: "auto", name: "Auto (Default)" },
            { value: "webrtc", name: "WebRTC" },
            { value: "websocket", name: "Web Socket" },
        ], {
            displayName: "Data Transport",
            preSelectedOption: (_k = settings === null || settings === void 0 ? void 0 : settings.dataTransport) !== null && _k !== void 0 ? _k : defaultSettings.dataTransport
        });
        this.dataTransport.addChangeListener(this.onSettingsChange.bind(this));
        this.dataTransport.mount(this.divElement);
        this.toggleFullscreenWithKeybind = new InputComponent("toggleFullscreenWithKeybind", "checkbox", "Toggle Fullscreen and Mouse Lock with Ctrl + Shift + I", {
            checked: settings === null || settings === void 0 ? void 0 : settings.toggleFullscreenWithKeybind
        });
        this.toggleFullscreenWithKeybind.addChangeListener(this.onSettingsChange.bind(this));
        this.toggleFullscreenWithKeybind.mount(this.divElement);
        this.onSettingsChange();
    }
    onSettingsChange() {
        if (this.videoSize.getValue() == "custom") {
            this.videoSizeWidth.setEnabled(true);
            this.videoSizeHeight.setEnabled(true);
        }
        else {
            this.videoSizeWidth.setEnabled(false);
            this.videoSizeHeight.setEnabled(false);
        }
        this.divElement.dispatchEvent(new ComponentEvent("ml-settingschange", this));
    }
    addChangeListener(listener) {
        this.divElement.addEventListener("ml-settingschange", listener);
    }
    removeChangeListener(listener) {
        this.divElement.removeEventListener("ml-settingschange", listener);
    }
    getStreamSettings() {
        const settings = defaultStreamSettings();
        settings.sidebarEdge = this.sidebarEdge.getValue();
        settings.bitrate = parseInt(this.bitrate.getValue());
        settings.packetSize = parseInt(this.packetSize.getValue());
        settings.fps = parseInt(this.fps.getValue());
        settings.videoSize = this.videoSize.getValue();
        settings.videoSizeCustom = {
            width: parseInt(this.videoSizeWidth.getValue()),
            height: parseInt(this.videoSizeHeight.getValue())
        };
        settings.videoFrameQueueSize = parseInt(this.videoSampleQueueSize.getValue());
        settings.videoCodec = this.videoCodec.getValue();
        settings.videoForceCodec = this.videoForceCodec.isChecked();
        settings.canvasRenderer = this.canvasRenderer.isChecked();
        settings.playAudioLocal = this.playAudioLocal.isChecked();
        settings.audioSampleQueueSize = parseInt(this.audioSampleQueueSize.getValue());
        settings.mouseScrollMode = this.mouseScrollMode.getValue();
        settings.controllerConfig.invertAB = this.controllerInvertAB.isChecked();
        settings.controllerConfig.invertXY = this.controllerInvertXY.isChecked();
        if (this.controllerSendIntervalOverride.isEnabled()) {
            settings.controllerConfig.sendIntervalOverride = parseInt(this.controllerSendIntervalOverride.getValue());
        }
        else {
            settings.controllerConfig.sendIntervalOverride = null;
        }
        settings.dataTransport = this.dataTransport.getValue();
        settings.toggleFullscreenWithKeybind = this.toggleFullscreenWithKeybind.isChecked();
        return settings;
    }
    mount(parent) {
        parent.appendChild(this.divElement);
    }
    unmount(parent) {
        parent.removeChild(this.divElement);
    }
}
