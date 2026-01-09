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
import { apiGetUser, apiLogout, apiPostUser, FetchError, getApi } from "./api.js";
import { showErrorPopup } from "./component/error.js";
import { setTouchContextMenuEnabled } from "./polyfill/ios_right_click.js";
import { UserList } from "./component/user/list.js";
import { AddUserModal } from "./component/user/add_modal.js";
import { showMessage, showModal } from "./component/modal/index.js";
import { buildUrl } from "./config_.js";
import { DetailedUserPage } from "./component/user/detailed_page.js";
function startApp() {
    return __awaiter(this, void 0, void 0, function* () {
        setTouchContextMenuEnabled(true);
        const api = yield getApi();
        checkPermissions(api);
        const rootElement = document.getElementById("root");
        if (rootElement == null) {
            showErrorPopup("couldn't find root element", true);
            return;
        }
        const app = new AdminApp(api);
        app.mount(rootElement);
        app.forceFetch();
    });
}
function checkPermissions(api) {
    return __awaiter(this, void 0, void 0, function* () {
        const user = yield apiGetUser(api);
        if (user.role != "Admin") {
            yield showMessage("You are not authorized to view this page!");
            window.location.href = buildUrl("/");
        }
    });
}
startApp();
class AdminApp {
    constructor(api) {
        this.root = document.createElement("div");
        // Top Line
        this.topLine = document.createElement("div");
        this.moonlightTextElement = document.createElement("h1");
        this.topLineActions = document.createElement("div");
        this.logoutButton = document.createElement("button");
        this.userButton = document.createElement("button");
        // Content
        this.content = document.createElement("div");
        // User Panel
        this.userPanel = document.createElement("div");
        this.addUserButton = document.createElement("button");
        this.userSearch = document.createElement("input");
        // User Info
        this.userInfoPage = null;
        this.api = api;
        // Top Line
        this.topLine.classList.add("top-line");
        this.moonlightTextElement.innerHTML =
            'Moonlight Web <span style="color:red; text-shadow: -1px -1px 0 #000, 1px -1px 0 #000, -1px 1px 0 #000, 1px 1px 0 #000; -webkit-text-stroke: 2px #000">Admin</span>';
        this.topLine.appendChild(this.moonlightTextElement);
        this.topLine.appendChild(this.topLineActions);
        this.topLineActions.classList.add("top-line-actions");
        this.logoutButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            yield apiLogout(this.api);
            window.location.reload();
        }));
        this.logoutButton.classList.add("logout-button");
        this.topLineActions.appendChild(this.logoutButton);
        this.userButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            window.location.href = buildUrl("/");
        }));
        this.userButton.classList.add("user-button");
        this.topLineActions.appendChild(this.userButton);
        this.root.appendChild(this.topLine);
        // Content div
        this.content.classList.add("admin-panel-content");
        this.root.appendChild(this.content);
        // Select User Panel
        this.userPanel.classList.add("user-panel");
        this.content.appendChild(this.userPanel);
        this.addUserButton.innerText = "Add User";
        this.addUserButton.addEventListener("click", () => __awaiter(this, void 0, void 0, function* () {
            var _a;
            const addUserModal = new AddUserModal();
            const userRequest = yield showModal(addUserModal);
            if (userRequest) {
                try {
                    const newUser = yield apiPostUser(this.api, userRequest);
                    this.userList.insertList(newUser.id, newUser);
                }
                catch (e) {
                    // 409 = Conflict
                    if (e instanceof FetchError && ((_a = e.getResponse()) === null || _a === void 0 ? void 0 : _a.status) == 409) {
                        // Name already exists
                        yield showMessage(`A user with the name "${userRequest.name}" already exists!`);
                    }
                    else {
                        throw e;
                    }
                }
            }
        }));
        this.userPanel.appendChild(this.addUserButton);
        this.userSearch.placeholder = "Search User";
        this.userSearch.type = "text";
        this.userSearch.addEventListener("input", this.onUserSearchChange.bind(this));
        this.userPanel.appendChild(this.userSearch);
        this.userList = new UserList(api);
        this.userList.addUserClickedListener(this.onUserClicked.bind(this));
        this.userList.addUserDeletedListener(this.onUserDeleted.bind(this));
        this.userList.mount(this.userPanel);
    }
    forceFetch() {
        return __awaiter(this, void 0, void 0, function* () {
            yield this.userList.forceFetch();
        });
    }
    onUserSearchChange() {
        this.userList.setFilter(this.userSearch.value);
    }
    onUserClicked(event) {
        return __awaiter(this, void 0, void 0, function* () {
            const user = yield apiGetUser(this.api, {
                user_id: event.component.getUserId(),
                name: null
            });
            this.setUserInfo(user);
        });
    }
    setUserInfo(user) {
        if (this.userInfoPage) {
            this.userInfoPage.unmount(this.content);
            this.userInfoPage.removeDeletedListener(this.onUserDeleted.bind(this));
        }
        this.userInfoPage = null;
        if (user) {
            this.userInfoPage = new DetailedUserPage(this.api, user);
            this.userInfoPage.addDeletedListener(this.onUserDeleted.bind(this));
            this.userInfoPage.mount(this.content);
        }
    }
    onUserDeleted(event) {
        var _a;
        if (((_a = this.userInfoPage) === null || _a === void 0 ? void 0 : _a.getUserId()) == event.component.getUserId()) {
            this.setUserInfo(null);
        }
        this.userList.removeUser(event.component.getUserId());
    }
    mount(parent) {
        parent.appendChild(this.root);
    }
    unmount(parent) {
        parent.removeChild(this.root);
    }
}
