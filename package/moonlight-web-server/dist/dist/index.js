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
import { getApi, apiPostHost, FetchError, apiLogout, apiGetUser, tryLogin, apiGetHost } from "./api.js";
import { AddHostModal } from "./component/host/add_modal.js";
import { HostList } from "./component/host/list.js";
import { showErrorPopup } from "./component/error.js";
import { showModal } from "./component/modal/index.js";
import { setContextMenu } from "./component/context_menu.js";
import { GameList } from "./component/game/list.js";
import { getLocalStreamSettings, setLocalStreamSettings, StreamSettingsComponent } from "./component/settings_menu.js";
import { setTouchContextMenuEnabled } from "./polyfill/ios_right_click.js";
import { buildUrl } from "./config_.js";
import { QRAuthModal, QRAuthCredentialsPrompt } from "./component/modal/qr_auth.js";
function startApp() {
    return __awaiter(this, void 0, void 0, function* () {
        setTouchContextMenuEnabled(true);
        const api = yield getApi();
        const rootElement = document.getElementById("root");
        if (rootElement == null) {
            showErrorPopup("couldn't find root element", true);
            return;
        }
        let lastAppState = null;
        if (sessionStorage) {
            const lastStateText = sessionStorage.getItem("mlState");
            if (lastStateText) {
                lastAppState = JSON.parse(lastStateText);
            }
        }
        const app = new MainApp(api);
        app.mount(rootElement);
        window.addEventListener("popstate", event => {
            app.setAppState(event.state, false);
        });
        app.forceFetch();
        if (lastAppState) {
            app.setAppState(lastAppState);
        }
    });
}
startApp();
function setAppState(state, pushHistory) {
    if (pushHistory) {
        history.pushState(state, "");
    }
    if (sessionStorage) {
        sessionStorage.setItem("mlState", JSON.stringify(state));
    }
}
function backAppState() {
    history.back();
}
class MainApp {
    constructor(api) {
        var _a;
        this.user = null;
        this.divElement = document.createElement("div");
        // Top Line
        this.topLine = document.createElement("div");
        this.moonlightTextElement = document.createElement("h1");
        this.topLineActions = document.createElement("div");
        this.logoutButton = document.createElement("button");
        // This is for the default user
        this.loginButton = document.createElement("button");
        this.adminButton = document.createElement("button");
        this.qrCodeButton = document.createElement("button");
        // Actions
        this.actionElement = document.createElement("div");
        this.backButton = document.createElement("button");
        this.hostAddButton = document.createElement("button");
        this.settingsButton = document.createElement("button");
        // Different submenus
        this.currentDisplay = null;
        this.gameList = null;
        this.api = api;
        // Top Line
        this.topLine.classList.add("top-line");
        this.moonlightTextElement.innerHTML = "Moonlight Web";
        this.topLine.appendChild(this.moonlightTextElement);
        this.topLine.appendChild(this.topLineActions);
        this.topLineActions.classList.add("top-line-actions");
        this.logoutButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            yield apiLogout(this.api);
            window.location.reload();
        }));
        this.logoutButton.classList.add("logout-button");
        this.loginButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            const success = yield tryLogin();
            if (success) {
                window.location.reload();
            }
        }));
        this.loginButton.classList.add("login-button");
        this.adminButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            window.location.href = buildUrl("/admin.html");
        }));
        this.adminButton.classList.add("admin-button");
        // QR Code button for mobile app login
        this.qrCodeButton.innerText = "📱 QR Code";
        this.qrCodeButton.title = "Show QR code for mobile app login";
        this.qrCodeButton.addEventListener("click", this.showQRCode.bind(this));
        this.qrCodeButton.classList.add("qr-code-button");
        // Actions
        this.actionElement.classList.add("actions-list");
        // Back button
        this.backButton.innerText = "Back";
        this.backButton.classList.add("button-fit-content");
        this.backButton.addEventListener("click", backAppState);
        // Host add button
        this.hostAddButton.classList.add("host-add");
        this.hostAddButton.addEventListener("click", this.addHost.bind(this));
        // Host list
        this.hostList = new HostList(api);
        this.hostList.addHostOpenListener(this.onHostOpen.bind(this));
        // Settings Button
        this.settingsButton.classList.add("open-settings");
        this.settingsButton.addEventListener("click", () => this.setCurrentDisplay("settings"));
        // Settings
        this.settings = new StreamSettingsComponent((_a = getLocalStreamSettings()) !== null && _a !== void 0 ? _a : undefined);
        this.settings.addChangeListener(this.onSettingsChange.bind(this));
        // Append default elements
        this.divElement.appendChild(this.topLine);
        this.divElement.appendChild(this.actionElement);
        this.setCurrentDisplay("hosts");
        // Context Menu
        document.body.addEventListener("contextmenu", this.onContextMenu.bind(this), { passive: false });
    }
    setAppState(state, pushIntoHistory) {
        if (state.display == "hosts") {
            this.setCurrentDisplay("hosts", null, pushIntoHistory);
        }
        else if (state.display == "games" && state.hostId != null) {
            this.setCurrentDisplay("games", { hostId: state.hostId }, pushIntoHistory);
        }
        else if (state.display == "settings") {
            this.setCurrentDisplay("settings", null, pushIntoHistory);
        }
    }
    addHost() {
        return __awaiter(this, void 0, void 0, function* () {
            const modal = new AddHostModal();
            let host = yield showModal(modal);
            if (host) {
                let newHost;
                try {
                    newHost = yield apiPostHost(this.api, host);
                }
                catch (e) {
                    if (e instanceof FetchError) {
                        const response = e.getResponse();
                        if (response && response.status == 404) {
                            showErrorPopup(`Host "${host.address}" is not reachable`);
                            return;
                        }
                    }
                    throw e;
                }
                this.hostList.insertList(newHost.host_id, newHost);
            }
        });
    }
    onContextMenu(event) {
        if (this.currentDisplay == "hosts" || this.currentDisplay == "games") {
            const elements = [
                {
                    name: "Reload",
                    callback: this.forceFetch.bind(this)
                }
            ];
            setContextMenu(event, {
                elements
            });
        }
    }
    onHostOpen(event) {
        return __awaiter(this, void 0, void 0, function* () {
            const hostId = event.component.getHostId();
            this.setCurrentDisplay("games", { hostId });
        });
    }
    onSettingsChange() {
        const newSettings = this.settings.getStreamSettings();
        setLocalStreamSettings(newSettings);
    }
    setCurrentDisplay(display, extraInfo, pushIntoHistory_) {
        var _a, _b, _c, _d, _e;
        const pushIntoHistory = pushIntoHistory_ === undefined ? true : pushIntoHistory_;
        if (display == "games" && (extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostId) == null) {
            // invalid input state
            throw "invalid display state was requested";
        }
        // Check if we need to change
        if (this.currentDisplay == display) {
            if (this.currentDisplay == "games" && ((_a = this.gameList) === null || _a === void 0 ? void 0 : _a.getHostId()) != (extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostId)) {
                // fall through
            }
            else {
                return;
            }
        }
        // Unmount the current display
        if (this.currentDisplay == "hosts") {
            this.actionElement.removeChild(this.hostAddButton);
            this.actionElement.removeChild(this.settingsButton);
            this.hostList.unmount(this.divElement);
        }
        else if (this.currentDisplay == "games") {
            this.actionElement.removeChild(this.backButton);
            this.actionElement.removeChild(this.settingsButton);
            (_b = this.gameList) === null || _b === void 0 ? void 0 : _b.unmount(this.divElement);
        }
        else if (this.currentDisplay == "settings") {
            this.actionElement.removeChild(this.backButton);
            this.settings.unmount(this.divElement);
        }
        // Mount the new display
        if (display == "hosts") {
            this.actionElement.appendChild(this.hostAddButton);
            this.actionElement.appendChild(this.settingsButton);
            this.hostList.mount(this.divElement);
            setAppState({ display: "hosts" }, pushIntoHistory);
        }
        else if (display == "games" && (extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostId) != null) {
            this.actionElement.appendChild(this.backButton);
            this.actionElement.appendChild(this.settingsButton);
            if (((_c = this.gameList) === null || _c === void 0 ? void 0 : _c.getHostId()) != (extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostId)) {
                this.gameList = new GameList(this.api, extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostId, (_d = extraInfo === null || extraInfo === void 0 ? void 0 : extraInfo.hostCache) !== null && _d !== void 0 ? _d : null);
                this.gameList.addForceReloadListener(this.forceFetch.bind(this));
            }
            this.gameList.mount(this.divElement);
            this.refreshGameListActiveGame();
            setAppState({ display: "games", hostId: (_e = this.gameList) === null || _e === void 0 ? void 0 : _e.getHostId() }, pushIntoHistory);
        }
        else if (display == "settings") {
            this.actionElement.appendChild(this.backButton);
            this.settings.mount(this.divElement);
            setAppState({ display: "settings" }, pushIntoHistory);
        }
        this.currentDisplay = display;
    }
    forceFetch() {
        return __awaiter(this, void 0, void 0, function* () {
            var _a;
            const promiseUser = this.refreshUserRole();
            yield Promise.all([
                this.hostList.forceFetch(),
                (_a = this.gameList) === null || _a === void 0 ? void 0 : _a.forceFetch()
            ]);
            if (this.currentDisplay == "games"
                && this.gameList
                && !this.hostList.getHost(this.gameList.getHostId())) {
                // The newly fetched list doesn't contain the hosts game view we're in -> go to hosts
                this.setCurrentDisplay("hosts");
            }
            yield Promise.all([
                promiseUser,
                this.refreshGameListActiveGame()
            ]);
        });
    }
    refreshUserRole() {
        return __awaiter(this, void 0, void 0, function* () {
            this.user = yield apiGetUser(this.api);
            if (this.topLineActions.contains(this.logoutButton)) {
                this.topLineActions.removeChild(this.logoutButton);
            }
            if (this.topLineActions.contains(this.loginButton)) {
                this.topLineActions.removeChild(this.loginButton);
            }
            if (this.topLineActions.contains(this.adminButton)) {
                this.topLineActions.removeChild(this.adminButton);
            }
            if (this.topLineActions.contains(this.qrCodeButton)) {
                this.topLineActions.removeChild(this.qrCodeButton);
            }
            if (this.user.is_default_user) {
                this.topLineActions.appendChild(this.loginButton);
            }
            else {
                // Show QR code button for logged-in users
                this.topLineActions.appendChild(this.qrCodeButton);
                this.topLineActions.appendChild(this.logoutButton);
            }
            if (this.user.role == "Admin") {
                this.topLineActions.appendChild(this.adminButton);
            }
        });
    }
    /**
     * Show QR code modal for mobile app authentication.
     * Prompts for password then displays QR code with credentials.
     */
    showQRCode() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!this.user || this.user.is_default_user) {
                showErrorPopup("Please log in first to generate a QR code");
                return;
            }
            // Prompt for password
            const credentialsPrompt = new QRAuthCredentialsPrompt(this.user.name);
            const credentials = yield showModal(credentialsPrompt);
            if (credentials) {
                // Show QR code modal with credentials
                const qrModal = new QRAuthModal(credentials.username, credentials.password);
                yield showModal(qrModal);
            }
        });
    }
    refreshGameListActiveGame() {
        return __awaiter(this, void 0, void 0, function* () {
            const gameList = this.gameList;
            const hostId = gameList === null || gameList === void 0 ? void 0 : gameList.getHostId();
            if (hostId == null) {
                return;
            }
            const host = this.hostList.getHost(hostId);
            let currentGame = null;
            if (host != null) {
                currentGame = yield host.getCurrentGame();
            }
            else {
                const host = yield apiGetHost(this.api, { host_id: hostId });
                if (host.current_game != 0) {
                    currentGame = host.current_game;
                }
            }
            if (currentGame != null) {
                gameList === null || gameList === void 0 ? void 0 : gameList.setActiveGame(currentGame);
            }
            else {
                gameList === null || gameList === void 0 ? void 0 : gameList.setActiveGame(null);
            }
        });
    }
    mount(parent) {
        parent.appendChild(this.divElement);
    }
    unmount(parent) {
        parent.removeChild(this.divElement);
    }
}
