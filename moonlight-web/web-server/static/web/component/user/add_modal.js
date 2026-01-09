import { InputComponent } from "../input.js";
import { FormModal } from "../modal/form.js";
import { createSelectRoleInput } from "./role_select.js";
export class AddUserModal extends FormModal {
    constructor() {
        super();
        this.header = document.createElement("h2");
        this.header.innerText = "User";
        this.name = new InputComponent("userName", "text", "Name", {
            formRequired: true
        });
        this.defaultPassword = new InputComponent("userPassword", "text", "Default Password", {
            formRequired: true
        });
        this.role = createSelectRoleInput("User");
        this.clientUniqueId = new InputComponent("userClientUniqueId", "text", "Moonlight Client Id", {
            formRequired: true,
            hasEnableCheckbox: true
        });
        this.name.addChangeListener(this.updateClientUniqueId.bind(this));
    }
    updateClientUniqueId() {
        this.clientUniqueId.setPlaceholder(this.name.getValue());
    }
    mountForm(form) {
        form.appendChild(this.header);
        this.name.mount(form);
        this.defaultPassword.mount(form);
        this.role.mount(form);
        this.clientUniqueId.mount(form);
    }
    reset() {
        this.name.reset();
        this.defaultPassword.reset();
        this.role.reset();
    }
    submit() {
        const name = this.name.getValue();
        const password = this.defaultPassword.getValue();
        const role = this.role.getValue();
        let clientUniqueId = name;
        if (this.clientUniqueId.isEnabled()) {
            clientUniqueId = this.clientUniqueId.getValue();
        }
        return {
            name,
            password,
            role,
            client_unique_id: clientUniqueId,
        };
    }
}
