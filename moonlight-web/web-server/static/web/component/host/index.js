var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
import { apiDeleteHost, apiGetHost, isDetailedHost, apiPostPair, apiWakeUp, apiGetUser, apiPatchHost } from "../../api.js";
import { ComponentEvent } from "../index.js";
import { setContextMenu } from "../context_menu.js";
import { showErrorPopup } from "../error.js";
import { showMessage } from "../modal/index.js";
import { HOST_IMAGE, HOST_OVERLAY_LOCK, HOST_OVERLAY_NONE, HOST_OVERLAY_OFFLINE } from "../../resources/index.js";
export class Host {
    constructor(api, hostId, host) {
        this.userCache = null;
        this.cache = null;
        this.divElement = document.createElement("div");
        this.imageElement = document.createElement("img");
        this.imageOverlayElement = document.createElement("img");
        this.nameElement = document.createElement("p");
        this.badgeElement = document.createElement("span");
        this.api = api;
        this.hostId = hostId;
        this.cache = host;
        // Configure image
        this.imageElement.classList.add("host-image");
        this.imageElement.src = HOST_IMAGE;
        // Configure image overlay
        this.imageOverlayElement.classList.add("host-image-overlay");
        // Configure name
        this.nameElement.classList.add("host-name");
        // Configure badge (for Backlight hosts)
        this.badgeElement.classList.add("host-badge");
        this.badgeElement.style.display = "none";
        // Append elements
        this.divElement.appendChild(this.imageElement);
        this.divElement.appendChild(this.imageOverlayElement);
        this.divElement.appendChild(this.nameElement);
        this.divElement.appendChild(this.badgeElement);
        this.divElement.addEventListener("click", this.onClick.bind(this));
        this.divElement.addEventListener("contextmenu", this.onContextMenu.bind(this));
        // Update cache
        if (host != null) {
            this.updateCache(host, null);
            apiGetUser(api).then((user) => this.userCache = user);
        }
        else {
            this.forceFetch();
        }
    }
    forceFetch() {
        return __awaiter(this, void 0, void 0, function* () {
            const [newCache, user] = yield Promise.all([
                apiGetHost(this.api, {
                    host_id: this.hostId,
                }),
                apiGetUser(this.api)
            ]);
            this.updateCache(newCache, user);
        });
    }
    getCurrentGame() {
        return __awaiter(this, void 0, void 0, function* () {
            yield this.forceFetch();
            if (this.cache && isDetailedHost(this.cache) && this.cache.current_game != 0) {
                return this.cache.current_game;
            }
            else {
                return null;
            }
        });
    }
    onClick(event) {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            if (((_a = this.cache) === null || _a === void 0 ? void 0 : _a.server_state) == null) {
                this.onContextMenu(event);
            }
            else if (((_b = this.cache) === null || _b === void 0 ? void 0 : _b.paired) == "Paired") {
                this.divElement.dispatchEvent(new ComponentEvent("ml-hostopen", this));
            }
            else {
                yield this.pair();
            }
        });
    }
    onContextMenu(event) {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j;
        const elements = [];
        if (((_a = this.cache) === null || _a === void 0 ? void 0 : _a.server_state) != null) {
            elements.push({
                name: "Show Details",
                callback: this.showDetails.bind(this),
            });
        }
        else if (((_b = this.cache) === null || _b === void 0 ? void 0 : _b.paired) == "Paired") {
            elements.push({
                name: "Send Wake Up Packet",
                callback: this.wakeUp.bind(this)
            });
        }
        elements.push({
            name: "Reload",
            callback: () => __awaiter(this, void 0, void 0, function* () { return this.forceFetch(); })
        });
        if (((_c = this.cache) === null || _c === void 0 ? void 0 : _c.server_state) != null && ((_d = this.cache) === null || _d === void 0 ? void 0 : _d.paired) == "NotPaired") {
            elements.push({
                name: "Pair",
                callback: this.pair.bind(this)
            });
        }
        // Make private / global
        if (((_e = this.userCache) === null || _e === void 0 ? void 0 : _e.role) == "Admin") {
            if (((_f = this.cache) === null || _f === void 0 ? void 0 : _f.owner) == "Global") {
                elements.push({
                    name: "Make Private",
                    callback: this.makePrivate.bind(this),
                    classes: ["context-menu-element-red"]
                });
            }
            else if (((_g = this.cache) === null || _g === void 0 ? void 0 : _g.owner) == "ThisUser") {
                elements.push({
                    name: "Make Global",
                    callback: this.makeGlobal.bind(this),
                    classes: ["context-menu-element-red"]
                });
            }
        }
        if (((_h = this.cache) === null || _h === void 0 ? void 0 : _h.owner) == "ThisUser" || ((_j = this.userCache) === null || _j === void 0 ? void 0 : _j.role) == "Admin") {
            elements.push({
                name: "Remove Host",
                callback: this.remove.bind(this)
            });
        }
        setContextMenu(event, {
            elements
        });
    }
    showDetails() {
        return __awaiter(this, void 0, void 0, function* () {
            let host = this.cache;
            if (!host || !isDetailedHost(host)) {
                host = yield apiGetHost(this.api, {
                    host_id: this.hostId,
                });
            }
            if (!host || !isDetailedHost(host)) {
                showErrorPopup(`failed to get details for host ${this.hostId}`);
                return;
            }
            this.updateCache(host, this.userCache);
            const hostTypeDisplay = host.host_type === "Backlight" ? "Backlight" :
                host.host_type === "Standard" ? "Standard Sunshine" : "Unknown";
            yield showMessage(`Web Id: ${host.host_id}\n` +
                `Name: ${host.name}\n` +
                `Host Type: ${hostTypeDisplay}\n` +
                `Pair Status: ${host.paired}\n` +
                `State: ${host.server_state}\n` +
                `Address: ${host.address}\n` +
                `Http Port: ${host.http_port}\n` +
                `Https Port: ${host.https_port}\n` +
                `External Port: ${host.external_port}\n` +
                `Version: ${host.version}\n` +
                `Gfe Version: ${host.gfe_version}\n` +
                `Unique ID: ${host.unique_id}\n` +
                `MAC: ${host.mac}\n` +
                `Local IP: ${host.local_ip}\n` +
                `Current Game: ${host.current_game}\n` +
                `Max Luma Pixels Hevc: ${host.max_luma_pixels_hevc}\n` +
                `Server Codec Mode Support: ${host.server_codec_mode_support}`);
        });
    }
    addHostRemoveListener(listener, options) {
        this.divElement.addEventListener("ml-hostremove", listener, options);
    }
    removeHostRemoveListener(listener, options) {
        this.divElement.removeEventListener("ml-hostremove", listener, options);
    }
    addHostOpenListener(listener, options) {
        this.divElement.addEventListener("ml-hostopen", listener, options);
    }
    removeHostOpenListener(listener, options) {
        this.divElement.removeEventListener("ml-hostopen", listener, options);
    }
    makeGlobal() {
        return __awaiter(this, void 0, void 0, function* () {
            yield apiPatchHost(this.api, {
                host_id: this.hostId,
                change_owner: true,
                owner: null,
            });
            if (this.cache) {
                this.cache.owner = "Global";
            }
        });
    }
    makePrivate() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            const user = (_a = this.userCache) !== null && _a !== void 0 ? _a : yield apiGetUser(this.api);
            yield apiPatchHost(this.api, {
                host_id: this.hostId,
                change_owner: true,
                owner: user.id,
            });
            if (this.cache) {
                this.cache.owner = "ThisUser";
            }
        });
    }
    remove() {
        return __awaiter(this, void 0, void 0, function* () {
            yield apiDeleteHost(this.api, {
                host_id: this.getHostId()
            });
            this.divElement.dispatchEvent(new ComponentEvent("ml-hostremove", this));
        });
    }
    wakeUp() {
        return __awaiter(this, void 0, void 0, function* () {
            yield apiWakeUp(this.api, {
                host_id: this.getHostId()
            });
            yield showMessage("Sent Wake Up packet. It might take a moment for your pc to start.");
        });
    }
    pair() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a, _b, _c, _d, _e;
            if (((_a = this.cache) === null || _a === void 0 ? void 0 : _a.paired) == "Paired") {
                yield this.forceFetch();
                if (((_b = this.cache) === null || _b === void 0 ? void 0 : _b.paired) == "Paired") {
                    showMessage("This host is already paired!");
                    return;
                }
            }
            const responseStream = yield apiPostPair(this.api, {
                host_id: this.getHostId()
            });
            const messageAbort = new AbortController();
            // Check if this is a Backlight host (auto-pairing) or standard Sunshine (PIN required)
            // Note: Unit variants serialize as strings, tuple variants as objects
            if (responseStream.response === "BacklightAutoPairing") {
                // Backlight host: auto-pairing in progress, no PIN needed
                showMessage(`Auto-pairing with Backlight host ${(_c = this.getCache()) === null || _c === void 0 ? void 0 : _c.name}...\nNo PIN required.`, { signal: messageAbort.signal });
            }
            else if (typeof responseStream.response === "object" && "Pin" in responseStream.response) {
                // Standard Sunshine: show PIN for manual entry
                showMessage(`Please pair your host ${(_d = this.getCache()) === null || _d === void 0 ? void 0 : _d.name} with this pin:\nPin: ${responseStream.response.Pin}`, { signal: messageAbort.signal });
            }
            else if (responseStream.response === "InternalServerError" || responseStream.response === "PairError") {
                throw `failed to pair (stage 1): ${responseStream.response}`;
            }
            else {
                throw `failed to pair: unexpected response format: ${JSON.stringify(responseStream.response)}`;
            }
            const resultResponse = yield responseStream.next();
            messageAbort.abort();
            if (!resultResponse) {
                throw "missing stage 2 of pairing";
            }
            else if (typeof resultResponse == "string") {
                throw `failed to pair (stage 2): ${resultResponse}`;
            }
            if (typeof resultResponse === "object" && "Paired" in resultResponse) {
                this.updateCache(resultResponse.Paired, null);
                // Show success message for Backlight auto-pairing
                if (responseStream.response === "BacklightAutoPairing") {
                    yield showMessage(`Successfully auto-paired with Backlight host ${(_e = this.getCache()) === null || _e === void 0 ? void 0 : _e.name}!`);
                }
            }
            else {
                throw `failed to pair: pairing error`;
            }
        });
    }
    getHostId() {
        return this.hostId;
    }
    getCache() {
        return this.cache;
    }
    updateCache(host, user) {
        if (this.getHostId() != host.host_id) {
            showErrorPopup(`tried to overwrite host ${this.getHostId()} with data from ${host.host_id}`);
            return;
        }
        if (this.cache == null) {
            this.cache = host;
        }
        else {
            // if server_state == null it means this host is offline
            // -> updating cache means setting it to offline
            if (this.cache.server_state != null) {
                Object.assign(this.cache, host);
            }
            else {
                this.cache = host;
            }
        }
        if (user) {
            this.userCache = user;
        }
        // Update Elements
        this.nameElement.innerText = this.cache.name;
        if (this.cache.server_state == null) {
            this.imageOverlayElement.src = HOST_OVERLAY_OFFLINE;
        }
        else if (this.cache.paired != "Paired") {
            this.imageOverlayElement.src = HOST_OVERLAY_LOCK;
        }
        else {
            this.imageOverlayElement.src = HOST_OVERLAY_NONE;
        }
        // Update badge for Backlight hosts
        if (isDetailedHost(this.cache) && this.cache.host_type === "Backlight") {
            this.badgeElement.textContent = "Backlight";
            this.badgeElement.style.display = "block";
        }
        else {
            this.badgeElement.style.display = "none";
        }
    }
    mount(parent) {
        parent.appendChild(this.divElement);
    }
    unmount(parent) {
        parent.removeChild(this.divElement);
    }
}
