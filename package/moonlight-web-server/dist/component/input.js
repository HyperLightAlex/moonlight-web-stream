import { ComponentEvent } from "./index.js";
export class ElementWithLabel {
    constructor(internalName, displayName) {
        this.div = document.createElement("div");
        this.label = document.createElement("label");
        if (displayName) {
            this.label.htmlFor = internalName;
            this.label.innerText = displayName;
            this.div.appendChild(this.label);
        }
    }
    mount(parent) {
        parent.appendChild(this.div);
    }
    unmount(parent) {
        parent.removeChild(this.div);
    }
}
export class InputComponent extends ElementWithLabel {
    constructor(internalName, type, displayName, init) {
        var _a, _b;
        super(internalName, displayName);
        this.fileLabel = null;
        this.numberSlider = null;
        this.inputEnabled = null;
        this.input = document.createElement("input");
        this.div.classList.add("input-div");
        this.input.id = internalName;
        this.input.type = type;
        if ((init === null || init === void 0 ? void 0 : init.defaultValue) != null) {
            this.input.defaultValue = init.defaultValue;
        }
        if ((init === null || init === void 0 ? void 0 : init.value) != null) {
            this.input.value = init.value;
        }
        if (init && init.checked != null) {
            this.input.checked = init.checked;
        }
        if (init && init.step != null) {
            this.input.step = init.step;
        }
        if (init && init.accept != null) {
            this.input.accept = init.accept;
        }
        if (init && init.inputMode != null) {
            this.input.inputMode = init.inputMode;
        }
        if (init && init.formRequired != null) {
            this.input.required = init.formRequired;
        }
        if (init && init.placeholer != null) {
            this.input.placeholder = init.placeholer;
        }
        if (type == "file") {
            this.fileLabel = document.createElement("div");
            this.fileLabel.innerText = this.label.innerText;
            this.fileLabel.classList.add("file-label");
            this.label.innerText = "Open File";
            this.label.classList.add("file-button");
            this.div.insertBefore(this.fileLabel, this.label);
        }
        if (init === null || init === void 0 ? void 0 : init.hasEnableCheckbox) {
            this.inputEnabled = document.createElement("input");
            this.inputEnabled.type = "checkbox";
            this.inputEnabled.defaultChecked = false;
            this.inputEnabled.addEventListener("change", () => {
                var _a, _b;
                this.setEnabled((_b = (_a = this.inputEnabled) === null || _a === void 0 ? void 0 : _a.checked) !== null && _b !== void 0 ? _b : (() => { throw "inputEnabled is null"; })());
                this.div.dispatchEvent(new ComponentEvent("ml-change", this));
            });
            this.div.appendChild(this.inputEnabled);
        }
        this.div.appendChild(this.input);
        this.input.addEventListener("change", () => {
            if (this.numberSlider) {
                this.numberSlider.value = this.input.value;
            }
            this.div.dispatchEvent(new ComponentEvent("ml-change", this));
        });
        if ((init === null || init === void 0 ? void 0 : init.numberSlider) && type != "number") {
            throw "tried to create InputComponent with number slider but type wasn't number";
        }
        if (type == "number" && (init === null || init === void 0 ? void 0 : init.numberSlider)) {
            this.numberSlider = document.createElement("input");
            this.numberSlider.type = "range";
            this.numberSlider.min = `${init.numberSlider.range_min}`;
            this.numberSlider.max = `${init.numberSlider.range_max}`;
            this.numberSlider.step = (_b = (_a = init.step) === null || _a === void 0 ? void 0 : _a.toString()) !== null && _b !== void 0 ? _b : "";
            this.numberSlider.addEventListener("change", () => {
                if (this.numberSlider) {
                    this.input.value = this.numberSlider.value;
                }
                else {
                    throw "failed to get value of number slider because it wasn't created";
                }
                this.div.dispatchEvent(new ComponentEvent("ml-change", this));
            });
            this.div.appendChild(this.numberSlider);
        }
        if (init === null || init === void 0 ? void 0 : init.hasEnableCheckbox) {
            // The main logic is further up
            this.setEnabled(false);
        }
    }
    reset() {
        this.input.value = "";
        if (this.numberSlider) {
            this.numberSlider.value = "";
        }
    }
    setValue(value) {
        this.input.value = value;
        if (this.numberSlider) {
            this.numberSlider.value = value;
        }
    }
    getValue() {
        return this.input.value;
    }
    isChecked() {
        return this.input.checked;
    }
    getFiles() {
        return this.input.files;
    }
    setEnabled(enabled) {
        if (this.inputEnabled) {
            this.inputEnabled.checked = enabled;
        }
        this.input.disabled = !enabled;
        if (this.numberSlider) {
            this.numberSlider.disabled = !enabled;
        }
    }
    isEnabled() {
        return !this.input.disabled;
    }
    addChangeListener(listener, options) {
        this.div.addEventListener("ml-change", listener, options);
    }
    removeChangeListener(listener) {
        this.div.removeEventListener("ml-change", listener);
    }
    setPlaceholder(newPlaceholder) {
        this.input.placeholder = newPlaceholder;
    }
    mount(parent) {
        super.mount(parent);
        if (this.numberSlider) {
            this.numberSlider.value = this.input.value;
        }
    }
}
export class SelectComponent extends ElementWithLabel {
    constructor(internalName, options, init) {
        super(internalName, init === null || init === void 0 ? void 0 : init.displayName);
        this.preSelectedOption = "";
        if (init && init.preSelectedOption) {
            this.preSelectedOption = init.preSelectedOption;
        }
        this.options = options;
        if (init && init.hasSearch && isElementSupported("datalist")) {
            this.strategy = "datalist";
            this.optionRoot = document.createElement("datalist");
            this.optionRoot.id = `${internalName}-list`;
            this.inputElement = document.createElement("input");
            this.inputElement.type = "text";
            this.inputElement.id = internalName;
            this.inputElement.setAttribute("list", this.optionRoot.id);
            if (init && init.preSelectedOption) {
                this.inputElement.defaultValue = init.preSelectedOption;
            }
            this.div.appendChild(this.inputElement);
            this.div.appendChild(this.optionRoot);
        }
        else {
            this.strategy = "select";
            this.inputElement = null;
            this.optionRoot = document.createElement("select");
            this.optionRoot.id = internalName;
            this.div.appendChild(this.optionRoot);
        }
        for (const option of options) {
            const optionElement = document.createElement("option");
            if (this.strategy == "datalist") {
                optionElement.value = option.name;
            }
            else if (this.strategy == "select") {
                optionElement.innerText = option.name;
                optionElement.value = option.value;
            }
            if (init && init.preSelectedOption == option.value) {
                optionElement.selected = true;
            }
            this.optionRoot.appendChild(optionElement);
        }
        this.optionRoot.addEventListener("change", () => {
            this.div.dispatchEvent(new ComponentEvent("ml-change", this));
        });
    }
    reset() {
        if (this.strategy == "datalist") {
            const inputElement = this.inputElement;
            inputElement.value = "";
        }
        else {
            const selectElement = this.optionRoot;
            selectElement.value = this.preSelectedOption;
        }
    }
    getValue() {
        var _a, _b;
        if (this.strategy == "datalist") {
            const name = this.inputElement.value;
            return (_b = (_a = this.options.find(option => option.name == name)) === null || _a === void 0 ? void 0 : _a.value) !== null && _b !== void 0 ? _b : "";
        }
        else if (this.strategy == "select") {
            return this.optionRoot.value;
        }
        throw "Invalid strategy for select input field";
    }
    setOptionEnabled(value, enabled) {
        for (const optionElement of this.optionRoot.options) {
            if (optionElement.value == value) {
                optionElement.disabled = !enabled;
            }
        }
    }
    addChangeListener(listener, options) {
        this.div.addEventListener("ml-change", listener, options);
    }
    removeChangeListener(listener) {
        this.div.removeEventListener("ml-change", listener);
    }
}
export function isElementSupported(tag) {
    // Create a test element for the tag
    const element = document.createElement(tag);
    // Check for support of custom elements registered via
    // `document.registerElement`
    if (tag.indexOf('-') > -1) {
        // Registered elements have their own constructor, while unregistered
        // ones use the `HTMLElement` or `HTMLUnknownElement` (if invalid name)
        // constructor (http://stackoverflow.com/a/28210364/1070244)
        return (element.constructor !== window.HTMLUnknownElement &&
            element.constructor !== window.HTMLElement);
    }
    // Obtain the element's internal [[Class]] property, if it doesn't 
    // match the `HTMLUnknownElement` interface than it must be supported
    return toString.call(element) !== '[object HTMLUnknownElement]';
}
;
