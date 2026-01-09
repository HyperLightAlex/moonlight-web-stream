import { SelectComponent } from "../input.js";
export function createSelectRoleInput(preselected) {
    return new SelectComponent("role", [
        { value: "User", name: "User" },
        { value: "Admin", name: "Admin" },
    ], {
        displayName: "Role",
        preSelectedOption: preselected
    });
}
