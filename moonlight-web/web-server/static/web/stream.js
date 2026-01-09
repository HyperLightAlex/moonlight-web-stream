var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
import "./polyfill/index.js";
import { getApi } from "./api.js";
import { showErrorPopup } from "./component/error.js";
import { Stream } from "./stream/index.js";
import { getModalBackground, showMessage, showModal } from "./component/modal/index.js";
import { getSidebarRoot, setSidebar, setSidebarExtended, setSidebarStyle } from "./component/sidebar/index.js";
import { defaultStreamInputConfig } from "./stream/input.js";
import { defaultStreamSettings, getLocalStreamSettings } from "./component/settings_menu.js";
import { SelectComponent } from "./component/input.js";
import { emptyVideoFormats, getSupportedVideoFormats, hasAnyCodec } from "./stream/video.js";
import { StreamKeys } from "./api_bindings.js";
import { ScreenKeyboard } from "./screen_keyboard.js";
import { FormModal } from "./component/modal/form.js";
import { streamStatsToText } from "./stream/stats.js";
function startApp() {
    return __awaiter(this, void 0, void 0, function* () {
        const api = yield getApi();
        const rootElement = document.getElementById("root");
        if (rootElement == null) {
            showErrorPopup("couldn't find root element", true);
            return;
        }
        // Get Host and App via Query
        const queryParams = new URLSearchParams(location.search);
        const hostIdStr = queryParams.get("hostId");
        const appIdStr = queryParams.get("appId");
        const hybridStr = queryParams.get("hybrid");
        if (hostIdStr == null || appIdStr == null) {
            yield showMessage("No Host or no App Id found");
            window.close();
            return;
        }
        const hostId = Number.parseInt(hostIdStr);
        const appId = Number.parseInt(appIdStr);
        const hybridMode = hybridStr === "true";
        if (hybridMode) {
            console.info("[Hybrid]: Hybrid streaming mode enabled - input handled by native client");
        }
        // event propagation on overlays
        const sidebarRoot = getSidebarRoot();
        if (sidebarRoot) {
            stopPropagationOn(sidebarRoot);
        }
        const modalBackground = getModalBackground();
        if (modalBackground) {
            stopPropagationOn(modalBackground);
        }
        // Start and Mount App
        const app = new ViewerApp(api, hostId, appId, hybridMode);
        app.mount(rootElement);
    });
}
// Prevent starting transition
window.requestAnimationFrame(() => {
    var _a;
    // Note: elements is a live array
    const elements = document.getElementsByClassName("prevent-start-transition");
    while (elements.length > 0) {
        (_a = elements.item(0)) === null || _a === void 0 ? void 0 : _a.classList.remove("prevent-start-transition");
    }
});
startApp();
class ViewerApp {
    constructor(api, hostId, appId, hybridMode = false) {
        var _a;
        this.div = document.createElement("div");
        this.statsDiv = document.createElement("div");
        this.stream = null;
        this.inputConfig = defaultStreamInputConfig();
        this.hasShownFullscreenEscapeWarning = false;
        this.isTogglingFullscreenWithKeybind = "none";
        this.api = api;
        this.hybridMode = hybridMode;
        // Configure sidebar (hide in hybrid mode as native handles input)
        this.sidebar = new ViewerSidebar(this);
        if (!hybridMode) {
            setSidebar(this.sidebar);
        }
        // Configure stats element
        this.statsDiv.hidden = true;
        this.statsDiv.classList.add("video-stats");
        setInterval(() => {
            var _a;
            // Update stats display every 100ms
            const stats = (_a = this.getStream()) === null || _a === void 0 ? void 0 : _a.getStats();
            if (stats && stats.isEnabled()) {
                this.statsDiv.hidden = false;
                const text = streamStatsToText(stats.getCurrentStats());
                this.statsDiv.innerText = text;
            }
            else {
                this.statsDiv.hidden = true;
            }
        }, 100);
        this.div.appendChild(this.statsDiv);
        // Configure stream
        const settings = (_a = getLocalStreamSettings()) !== null && _a !== void 0 ? _a : defaultStreamSettings();
        let browserWidth = Math.max(document.documentElement.clientWidth || 0, window.innerWidth || 0);
        let browserHeight = Math.max(document.documentElement.clientHeight || 0, window.innerHeight || 0);
        this.previousMouseMode = this.inputConfig.mouseMode;
        this.toggleFullscreenWithKeybind = settings.toggleFullscreenWithKeybind;
        this.startStream(hostId, appId, settings, [browserWidth, browserHeight]);
        this.settings = settings;
        // Configure input (skip in hybrid mode - native client handles it)
        if (!hybridMode) {
            this.addListeners(document);
            this.addListeners(document.getElementById("input"));
        }
        // Skip input-related event listeners in hybrid mode
        if (!hybridMode) {
            window.addEventListener("blur", () => {
                var _a;
                (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().raiseAllKeys();
            });
            document.addEventListener("visibilitychange", () => {
                var _a;
                if (document.visibilityState !== "visible") {
                    (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().raiseAllKeys();
                }
            });
            document.addEventListener("pointerlockchange", this.onPointerLockChange.bind(this));
            document.addEventListener("fullscreenchange", this.onFullscreenChange.bind(this));
            window.addEventListener("gamepadconnected", this.onGamepadConnect.bind(this));
            window.addEventListener("gamepaddisconnected", this.onGamepadDisconnect.bind(this));
            // Connect all gamepads
            for (const gamepad of navigator.getGamepads()) {
                if (gamepad != null) {
                    this.onGamepadAdd(gamepad);
                }
            }
        }
    }
    addListeners(element) {
        element.addEventListener("keydown", this.onKeyDown.bind(this), { passive: false });
        element.addEventListener("keyup", this.onKeyUp.bind(this), { passive: false });
        element.addEventListener("mousedown", this.onMouseButtonDown.bind(this), { passive: false });
        element.addEventListener("mouseup", this.onMouseButtonUp.bind(this), { passive: false });
        element.addEventListener("mousemove", this.onMouseMove.bind(this), { passive: false });
        element.addEventListener("wheel", this.onMouseWheel.bind(this), { passive: false });
        element.addEventListener("contextmenu", this.onContextMenu.bind(this), { passive: false });
        element.addEventListener("touchstart", this.onTouchStart.bind(this), { passive: false });
        element.addEventListener("touchend", this.onTouchEnd.bind(this), { passive: false });
        element.addEventListener("touchcancel", this.onTouchCancel.bind(this), { passive: false });
        element.addEventListener("touchmove", this.onTouchMove.bind(this), { passive: false });
    }
    startStream(hostId, appId, settings, browserSize) {
        return __awaiter(this, void 0, void 0, function* () {
            setSidebarStyle({
                edge: settings.sidebarEdge,
            });
            let supportedVideoFormats = yield getSupportedVideoFormats();
            if (settings.videoCodec == "h264") {
                const newSupportedVideoFormats = emptyVideoFormats();
                newSupportedVideoFormats.H264 = supportedVideoFormats.H264;
                newSupportedVideoFormats.H264_HIGH8_444 = supportedVideoFormats.H264_HIGH8_444;
                if (settings.videoForceCodec) {
                    newSupportedVideoFormats.H264 = true;
                }
                supportedVideoFormats = newSupportedVideoFormats;
            }
            else if (settings.videoCodec == "h265") {
                const newSupportedVideoFormats = emptyVideoFormats();
                newSupportedVideoFormats.H265 = supportedVideoFormats.H265;
                newSupportedVideoFormats.H265_MAIN10 = supportedVideoFormats.H265_MAIN10;
                newSupportedVideoFormats.H265_REXT8_444 = supportedVideoFormats.H265_REXT8_444;
                newSupportedVideoFormats.H265_REXT10_444 = supportedVideoFormats.H265_REXT10_444;
                if (settings.videoForceCodec) {
                    newSupportedVideoFormats.H265 = true;
                }
                supportedVideoFormats = newSupportedVideoFormats;
            }
            else if (settings.videoCodec == "av1") {
                const newSupportedVideoFormats = emptyVideoFormats();
                newSupportedVideoFormats.AV1 = supportedVideoFormats.AV1;
                newSupportedVideoFormats.AV1_MAIN8 = supportedVideoFormats.AV1_MAIN8;
                newSupportedVideoFormats.AV1_MAIN10 = supportedVideoFormats.AV1_MAIN10;
                newSupportedVideoFormats.AV1_REXT8_444 = supportedVideoFormats.AV1_REXT8_444;
                newSupportedVideoFormats.AV1_REXT10_444 = supportedVideoFormats.AV1_REXT10_444;
                if (settings.videoForceCodec) {
                    newSupportedVideoFormats.AV1 = true;
                }
                supportedVideoFormats = newSupportedVideoFormats;
            }
            else if (settings.videoCodec == "auto") {
                // do nothing
            }
            if (!settings.videoForceCodec) {
                // TODO: make this and the standard supported video formats
                supportedVideoFormats.H264 = true;
            }
            else {
                // TODO: use logger
                console.info(`Forcing the video codec!`);
            }
            if (!hasAnyCodec(supportedVideoFormats)) {
                // TODO: use the logger and log via fatal
                throw "Couldn't find any supported video format. Change the codec option to H264 in the settings if you're unsure which codecs are supported.";
            }
            this.stream = new Stream(this.api, hostId, appId, settings, supportedVideoFormats, browserSize, this.hybridMode);
            // Add app info listener
            this.stream.addInfoListener(this.onInfo.bind(this));
            // In hybrid mode, don't show the connection modal - Android has its own loading UI
            if (!this.hybridMode) {
                // Create connection info modal
                const connectionInfo = new ConnectionInfoModal();
                this.stream.addInfoListener(connectionInfo.onInfo.bind(connectionInfo));
                showModal(connectionInfo);
            }
            else {
                // In hybrid mode, hide all UI elements and only show video
                document.body.classList.add("hybrid-mode");
            }
            // Start animation frame loop
            this.onTouchUpdate();
            this.onGamepadUpdate();
            this.stream.getInput().addScreenKeyboardVisibleEvent(this.onScreenKeyboardSetVisible.bind(this));
            this.stream.mount(this.div);
        });
    }
    onInfo(event) {
        return __awaiter(this, void 0, void 0, function* () {
            const data = event.detail;
            if (data.type == "app") {
                const app = data.app;
                document.title = `Stream: ${app.title}`;
            }
            else if (data.type == "connectionComplete") {
                this.sidebar.onCapabilitiesChange(data.capabilities);
            }
        });
    }
    focusInput() {
        var _a;
        if (((_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().getCurrentPredictedTouchAction()) != "screenKeyboard" && !this.sidebar.getScreenKeyboard().isVisible()) {
            const inputElement = document.getElementById("input");
            inputElement.focus();
        }
    }
    onUserInteraction() {
        var _a, _b, _c, _d;
        this.focusInput();
        (_b = (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getVideoRenderer()) === null || _b === void 0 ? void 0 : _b.onUserInteraction();
        (_d = (_c = this.stream) === null || _c === void 0 ? void 0 : _c.getAudioPlayer()) === null || _d === void 0 ? void 0 : _d.onUserInteraction();
    }
    onScreenKeyboardSetVisible(event) {
        console.info(event.detail);
        const screenKeyboard = this.sidebar.getScreenKeyboard();
        const newShown = event.detail.visible;
        if (newShown != screenKeyboard.isVisible()) {
            if (newShown) {
                screenKeyboard.show();
            }
            else {
                screenKeyboard.hide();
            }
        }
    }
    // Input
    getInputConfig() {
        return this.inputConfig;
    }
    setInputConfig(config) {
        var _a;
        Object.assign(this.inputConfig, config);
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().setConfig(this.inputConfig);
    }
    // Keyboard
    onKeyDown(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onKeyDown(event);
        event.stopPropagation();
    }
    onKeyUp(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onKeyUp(event);
        event.stopPropagation();
        if (this.toggleFullscreenWithKeybind && this.isTogglingFullscreenWithKeybind == "none" && event.ctrlKey && event.shiftKey && event.code == "KeyI") {
            this.isTogglingFullscreenWithKeybind = "waitForCtrl";
        }
        if (this.isTogglingFullscreenWithKeybind == "waitForCtrl" && (event.code == "ControlRight" || event.code == "ControlLeft")) {
            this.isTogglingFullscreenWithKeybind = "makingFullscreen";
            (() => __awaiter(this, void 0, void 0, function* () {
                if (this.isFullscreen()) {
                    yield this.exitPointerLock();
                    yield this.exitFullscreen();
                }
                else {
                    yield this.requestFullscreen();
                    yield this.requestPointerLock();
                }
                this.isTogglingFullscreenWithKeybind = "none";
            }))();
        }
    }
    // Mouse
    onMouseButtonDown(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onMouseDown(event, this.getStreamRect());
        event.stopPropagation();
    }
    onMouseButtonUp(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onMouseUp(event);
        event.stopPropagation();
    }
    onMouseMove(event) {
        var _a;
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onMouseMove(event, this.getStreamRect());
        event.stopPropagation();
    }
    onMouseWheel(event) {
        var _a;
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onMouseWheel(event);
        event.stopPropagation();
    }
    onContextMenu(event) {
        event.preventDefault();
        event.stopPropagation();
    }
    // Touch
    onTouchStart(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onTouchStart(event, this.getStreamRect());
        event.stopPropagation();
    }
    onTouchEnd(event) {
        var _a;
        this.onUserInteraction();
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onTouchEnd(event, this.getStreamRect());
        event.stopPropagation();
    }
    onTouchCancel(event) {
        var _a;
        this.onUserInteraction();
        event === null || event === void 0 ? void 0 : event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onTouchCancel(event, this.getStreamRect());
        event.stopPropagation();
    }
    onTouchUpdate() {
        var _a;
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onTouchUpdate(this.getStreamRect());
        window.requestAnimationFrame(this.onTouchUpdate.bind(this));
    }
    onTouchMove(event) {
        var _a;
        event.preventDefault();
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onTouchMove(event, this.getStreamRect());
        event.stopPropagation();
    }
    // Gamepad
    onGamepadConnect(event) {
        this.onGamepadAdd(event.gamepad);
    }
    onGamepadAdd(gamepad) {
        var _a;
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onGamepadConnect(gamepad);
    }
    onGamepadDisconnect(event) {
        var _a;
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onGamepadDisconnect(event);
    }
    onGamepadUpdate() {
        var _a;
        (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getInput().onGamepadUpdate();
        window.requestAnimationFrame(this.onGamepadUpdate.bind(this));
    }
    // Fullscreen
    requestFullscreen() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            const body = document.body;
            if (body) {
                if (!("requestFullscreen" in body && typeof body.requestFullscreen == "function")) {
                    yield showMessage("Fullscreen is not supported by your browser!");
                    return;
                }
                this.focusInput();
                if (!this.isFullscreen()) {
                    try {
                        yield body.requestFullscreen({
                            navigationUI: "hide"
                        });
                    }
                    catch (e) {
                        console.warn("failed to request fullscreen", e);
                    }
                }
                if ("keyboard" in navigator && navigator.keyboard && "lock" in navigator.keyboard) {
                    yield navigator.keyboard.lock();
                    if (!this.hasShownFullscreenEscapeWarning) {
                        yield showMessage("To exit Fullscreen you'll have to hold ESC for a few seconds.");
                    }
                    this.hasShownFullscreenEscapeWarning = true;
                }
                if (((_a = this.getStream()) === null || _a === void 0 ? void 0 : _a.getInput().getConfig().mouseMode) == "relative") {
                    yield this.requestPointerLock();
                }
                try {
                    if (screen && "orientation" in screen) {
                        const orientation = screen.orientation;
                        if ("lock" in orientation && typeof orientation.lock == "function") {
                            yield orientation.lock("landscape");
                        }
                    }
                }
                catch (e) {
                    console.warn("failed to set orientation to landscape", e);
                }
            }
            else {
                console.warn("root element not found");
            }
        });
    }
    exitFullscreen() {
        return __awaiter(this, void 0, void 0, function* () {
            if ("keyboard" in navigator && navigator.keyboard && "unlock" in navigator.keyboard) {
                yield navigator.keyboard.unlock();
            }
            if ("exitFullscreen" in document && typeof document.exitFullscreen == "function") {
                yield document.exitFullscreen();
            }
        });
    }
    isFullscreen() {
        return "fullscreenElement" in document && !!document.fullscreenElement;
    }
    onFullscreenChange() {
        return __awaiter(this, void 0, void 0, function* () {
            this.checkFullyImmersed();
        });
    }
    // Pointer Lock
    requestPointerLock() {
        return __awaiter(this, arguments, void 0, function* (errorIfNotFound = false) {
            this.previousMouseMode = this.inputConfig.mouseMode;
            const inputElement = document.getElementById("input");
            if (inputElement && "requestPointerLock" in inputElement && typeof inputElement.requestPointerLock == "function") {
                this.focusInput();
                this.inputConfig.mouseMode = "relative";
                this.setInputConfig(this.inputConfig);
                setSidebarExtended(false);
                const onLockError = () => {
                    document.removeEventListener("pointerlockerror", onLockError);
                    // Fallback: try to request pointer lock without options
                    inputElement.requestPointerLock();
                };
                document.addEventListener("pointerlockerror", onLockError, { once: true });
                try {
                    let promise = inputElement.requestPointerLock({
                        unadjustedMovement: true
                    });
                    if (promise) {
                        yield promise;
                    }
                    else {
                        inputElement.requestPointerLock();
                    }
                }
                catch (error) {
                    // Some platforms do not support unadjusted movement. If you
                    // would like PointerLock anyway, request again.
                    if (error instanceof Error && error.name == "NotSupportedError") {
                        inputElement.requestPointerLock();
                    }
                    else {
                        throw error;
                    }
                }
                finally {
                    document.removeEventListener("pointerlockerror", onLockError);
                }
            }
            else if (errorIfNotFound) {
                yield showMessage("Pointer Lock not supported");
            }
        });
    }
    exitPointerLock() {
        return __awaiter(this, void 0, void 0, function* () {
            if ("exitPointerLock" in document && typeof document.exitPointerLock == "function") {
                document.exitPointerLock();
            }
        });
    }
    onPointerLockChange() {
        this.checkFullyImmersed();
        if (!document.pointerLockElement) {
            this.inputConfig.mouseMode = this.previousMouseMode;
            this.setInputConfig(this.inputConfig);
        }
    }
    // -- Fully immersed Fullscreen -> Fullscreen API + Pointer Lock
    checkFullyImmersed() {
        if ("pointerLockElement" in document && document.pointerLockElement &&
            "fullscreenElement" in document && document.fullscreenElement) {
            // We're fully immersed -> remove sidebar
            setSidebar(null);
        }
        else {
            setSidebar(this.sidebar);
        }
    }
    mount(parent) {
        parent.appendChild(this.div);
    }
    unmount(parent) {
        parent.removeChild(this.div);
    }
    getStreamRect() {
        var _a, _b, _c;
        // The bounding rect of the videoElement or canvasElement can be bigger than the actual video
        // -> We need to correct for this when sending positions, else positions are wrong
        return (_c = (_b = (_a = this.stream) === null || _a === void 0 ? void 0 : _a.getVideoRenderer()) === null || _b === void 0 ? void 0 : _b.getStreamRect()) !== null && _c !== void 0 ? _c : new DOMRect();
    }
    getStream() {
        return this.stream;
    }
}
class ConnectionInfoModal {
    constructor() {
        this.eventTarget = new EventTarget();
        this.root = document.createElement("div");
        this.text = document.createElement("p");
        this.debugDetailButton = document.createElement("button");
        this.debugDetail = ""; // We store this seperate because line breaks don't work when the element is not mounted on the dom
        this.debugDetailDisplay = document.createElement("div");
        this.root.classList.add("modal-video-connect");
        this.text.innerText = "Connecting";
        this.root.appendChild(this.text);
        this.debugDetailButton.innerText = "Show Logs";
        this.debugDetailButton.addEventListener("click", this.onDebugDetailClick.bind(this));
        this.root.appendChild(this.debugDetailButton);
        this.debugDetailDisplay.classList.add("textlike");
        this.debugDetailDisplay.classList.add("modal-video-connect-debug");
    }
    onDebugDetailClick() {
        let debugDetailCurrentlyShown = this.root.contains(this.debugDetailDisplay);
        if (debugDetailCurrentlyShown) {
            this.debugDetailButton.innerText = "Show Logs";
            this.root.removeChild(this.debugDetailDisplay);
        }
        else {
            this.debugDetailButton.innerText = "Hide Logs";
            this.root.appendChild(this.debugDetailDisplay);
            this.debugDetailDisplay.innerText = this.debugDetail;
        }
    }
    debugLog(line) {
        this.debugDetail += `${line}\n`;
        this.debugDetailDisplay.innerText = this.debugDetail;
        console.info(`[Stream]: ${line}`);
    }
    onInfo(event) {
        const data = event.detail;
        if (data.type == "stageStarting") {
            const text = `Server: Starting Stage: ${data.stage}`;
            this.text.innerText = text;
            this.debugLog(text);
        }
        else if (data.type == "stageComplete") {
            const text = `Server: Completed Stage: ${data.stage}`;
            this.text.innerText = text;
            this.debugLog(text);
        }
        else if (data.type == "stageFailed") {
            const text = `Server: Failed Stage: ${data.stage} with error ${data.errorCode}`;
            this.text.innerText = text;
            this.debugLog(text);
        }
        else if (data.type == "connectionComplete") {
            const text = `Connection Complete`;
            this.text.innerText = text;
            this.debugLog(text);
            this.eventTarget.dispatchEvent(new Event("ml-connected"));
        }
        else if (data.type == "addDebugLine") {
            const message = data.line.trim();
            if (message) {
                this.debugLog(message);
            }
            if (data.additional == "fatal") {
                showModal(this);
            }
            else if (data.additional == "recover") {
                showModal(null);
            }
        }
        else if (data.type == "serverMessage") {
            const text = `Server: ${data.message}`;
            this.text.innerText = text;
            this.debugLog(text);
        }
        else if (data.type == "connectionTerminated") {
            const text = `Server: Connection Terminated with code ${data.errorCode}`;
            this.text.innerText = text;
            this.debugLog(text);
            showModal(this);
        }
    }
    onFinish(abort) {
        return new Promise((resolve, reject) => {
            this.eventTarget.addEventListener("ml-connected", () => resolve(), { once: true, signal: abort });
        });
    }
    mount(parent) {
        parent.appendChild(this.root);
    }
    unmount(parent) {
        parent.removeChild(this.root);
    }
}
class ViewerSidebar {
    constructor(app) {
        this.div = document.createElement("div");
        this.buttonDiv = document.createElement("div");
        this.sendKeycodeButton = document.createElement("button");
        this.keyboardButton = document.createElement("button");
        this.screenKeyboard = new ScreenKeyboard();
        this.lockMouseButton = document.createElement("button");
        this.fullscreenButton = document.createElement("button");
        this.statsButton = document.createElement("button");
        this.app = app;
        // Configure divs
        this.div.classList.add("sidebar-stream");
        this.buttonDiv.classList.add("sidebar-stream-buttons");
        this.div.appendChild(this.buttonDiv);
        // Send keycode
        this.sendKeycodeButton.innerText = "Send Keycode";
        this.sendKeycodeButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            const key = yield showModal(new SendKeycodeModal());
            if (key == null) {
                return;
            }
            (_a = this.app.getStream()) === null || _a === void 0 ? void 0 : _a.getInput().sendKey(true, key, 0);
            (_b = this.app.getStream()) === null || _b === void 0 ? void 0 : _b.getInput().sendKey(false, key, 0);
        }));
        this.buttonDiv.appendChild(this.sendKeycodeButton);
        // Pointer Lock
        this.lockMouseButton.innerText = "Lock Mouse";
        this.lockMouseButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            yield this.app.requestPointerLock(true);
        }));
        this.buttonDiv.appendChild(this.lockMouseButton);
        // Pop up keyboard
        this.keyboardButton.innerText = "Keyboard";
        this.keyboardButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            setSidebarExtended(false);
            this.screenKeyboard.show();
        }));
        this.buttonDiv.appendChild(this.keyboardButton);
        this.screenKeyboard.addKeyDownListener(this.onKeyDown.bind(this));
        this.screenKeyboard.addKeyUpListener(this.onKeyUp.bind(this));
        this.screenKeyboard.addTextListener(this.onText.bind(this));
        this.div.appendChild(this.screenKeyboard.getHiddenElement());
        // Fullscreen
        this.fullscreenButton.innerText = "Fullscreen";
        this.fullscreenButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            if (this.app.isFullscreen()) {
                yield this.app.exitFullscreen();
            }
            else {
                yield this.app.requestFullscreen();
            }
        }));
        this.buttonDiv.appendChild(this.fullscreenButton);
        // Stats
        this.statsButton.innerText = "Stats";
        this.statsButton.addEventListener("click", () => {
            var _a;
            const stats = (_a = this.app.getStream()) === null || _a === void 0 ? void 0 : _a.getStats();
            if (stats) {
                stats.toggle();
            }
        });
        this.buttonDiv.appendChild(this.statsButton);
        // Select Mouse Mode
        this.mouseMode = new SelectComponent("mouseMode", [
            { value: "relative", name: "Relative" },
            { value: "follow", name: "Follow" },
            { value: "pointAndDrag", name: "Point and Drag" }
        ], {
            displayName: "Mouse Mode",
            preSelectedOption: this.app.getInputConfig().mouseMode
        });
        this.mouseMode.addChangeListener(this.onMouseModeChange.bind(this));
        this.mouseMode.mount(this.div);
        // Select Touch Mode
        this.touchMode = new SelectComponent("touchMode", [
            { value: "touch", name: "Touch" },
            { value: "mouseRelative", name: "Relative" },
            { value: "pointAndDrag", name: "Point and Drag" }
        ], {
            displayName: "Touch Mode",
            preSelectedOption: this.app.getInputConfig().touchMode
        });
        this.touchMode.addChangeListener(this.onTouchModeChange.bind(this));
        this.touchMode.mount(this.div);
    }
    onCapabilitiesChange(capabilities) {
        this.touchMode.setOptionEnabled("touch", capabilities.touch);
    }
    getScreenKeyboard() {
        return this.screenKeyboard;
    }
    // -- Keyboard
    onText(event) {
        var _a;
        (_a = this.app.getStream()) === null || _a === void 0 ? void 0 : _a.getInput().sendText(event.detail.text);
    }
    onKeyDown(event) {
        var _a;
        (_a = this.app.getStream()) === null || _a === void 0 ? void 0 : _a.getInput().onKeyDown(event);
    }
    onKeyUp(event) {
        var _a;
        (_a = this.app.getStream()) === null || _a === void 0 ? void 0 : _a.getInput().onKeyUp(event);
    }
    // -- Mouse Mode
    onMouseModeChange() {
        const config = this.app.getInputConfig();
        config.mouseMode = this.mouseMode.getValue();
        this.app.setInputConfig(config);
    }
    // -- Touch Mode
    onTouchModeChange() {
        const config = this.app.getInputConfig();
        config.touchMode = this.touchMode.getValue();
        this.app.setInputConfig(config);
    }
    extended() {
    }
    unextend() {
    }
    mount(parent) {
        parent.appendChild(this.div);
    }
    unmount(parent) {
        parent.removeChild(this.div);
    }
}
class SendKeycodeModal extends FormModal {
    constructor() {
        super();
        const keyList = [];
        for (const keyNameRaw in StreamKeys) {
            const keyName = keyNameRaw;
            const keyValue = StreamKeys[keyName];
            const PREFIX = "VK_";
            let name = keyName;
            if (name.startsWith(PREFIX)) {
                name = name.slice(PREFIX.length);
            }
            keyList.push({
                value: keyValue.toString(),
                name
            });
        }
        this.dropdownSearch = new SelectComponent("winKeycode", keyList, {
            hasSearch: true,
            displayName: "Select Keycode"
        });
    }
    mountForm(form) {
        this.dropdownSearch.mount(form);
    }
    reset() {
        this.dropdownSearch.reset();
    }
    submit() {
        const keyString = this.dropdownSearch.getValue();
        if (keyString == null) {
            return null;
        }
        return parseInt(keyString);
    }
}
// Stop propagation so the stream doesn't get it
function stopPropagationOn(element) {
    element.addEventListener("keydown", onStopPropagation);
    element.addEventListener("keyup", onStopPropagation);
    element.addEventListener("keypress", onStopPropagation);
    element.addEventListener("click", onStopPropagation);
    element.addEventListener("mousedown", onStopPropagation);
    element.addEventListener("mouseup", onStopPropagation);
    element.addEventListener("mousemove", onStopPropagation);
    element.addEventListener("wheel", onStopPropagation);
    element.addEventListener("contextmenu", onStopPropagation);
    element.addEventListener("touchstart", onStopPropagation);
    element.addEventListener("touchmove", onStopPropagation);
    element.addEventListener("touchend", onStopPropagation);
    element.addEventListener("touchcancel", onStopPropagation);
}
function onStopPropagation(event) {
    event.stopPropagation();
}
