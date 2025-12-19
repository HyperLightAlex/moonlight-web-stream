import { FormModal } from "./form.js"

/**
 * QR Code Authentication Modal.
 * 
 * Displays a QR code containing the user's credentials for scanning
 * by the Backbone mobile app. The QR code contains a JSON payload with:
 * - type: "moonlight_auth"
 * - v: version number (1)
 * - u: username
 * - p: password
 */
export class QRAuthModal extends FormModal<void> {
    private container: HTMLDivElement = document.createElement("div")
    private title: HTMLHeadingElement = document.createElement("h2")
    private description: HTMLParagraphElement = document.createElement("p")
    private qrContainer: HTMLDivElement = document.createElement("div")
    private qrImage: HTMLImageElement = document.createElement("img")
    private instructions: HTMLParagraphElement = document.createElement("p")
    private warningText: HTMLParagraphElement = document.createElement("p")

    private username: string
    private password: string

    constructor(username: string, password: string) {
        super()

        this.username = username
        this.password = password

        this.container.classList.add("qr-auth-modal")

        this.title.innerText = "Mobile App Login"
        this.title.style.marginBottom = "8px"

        this.description.innerText = "Scan this QR code with the Backbone app to login automatically."
        this.description.style.color = "#888"
        this.description.style.marginBottom = "16px"

        this.qrContainer.style.display = "flex"
        this.qrContainer.style.justifyContent = "center"
        this.qrContainer.style.alignItems = "center"
        this.qrContainer.style.padding = "16px"
        this.qrContainer.style.backgroundColor = "#ffffff"
        this.qrContainer.style.borderRadius = "12px"
        this.qrContainer.style.marginBottom = "16px"

        this.qrImage.style.width = "200px"
        this.qrImage.style.height = "200px"
        this.qrImage.alt = "QR Code for mobile login"

        this.instructions.innerHTML = `
            <strong>Instructions:</strong><br>
            1. Open the Backbone app on your phone<br>
            2. Select this PC from the server list<br>
            3. Point your camera at this QR code
        `
        this.instructions.style.fontSize = "14px"
        this.instructions.style.lineHeight = "1.6"
        this.instructions.style.color = "#666"
        this.instructions.style.marginBottom = "16px"

        this.warningText.innerHTML = "⚠️ This QR code contains your login credentials. Do not share it."
        this.warningText.style.fontSize = "12px"
        this.warningText.style.color = "#ff9800"
        this.warningText.style.textAlign = "center"

        this.qrContainer.appendChild(this.qrImage)
    }

    private generateQRPayload(): string {
        const payload = {
            type: "moonlight_auth",
            v: 1,
            u: this.username,
            p: this.password
        }
        return JSON.stringify(payload)
    }

    private generateQRCodeUrl(): string {
        const payload = this.generateQRPayload()
        // Use Google Charts API for QR code generation (simple, no dependencies)
        // Note: For production, consider using a local QR code library
        const encoded = encodeURIComponent(payload)
        return `https://api.qrserver.com/v1/create-qr-code/?size=200x200&data=${encoded}`
    }

    reset(): void {
        // Nothing to reset
    }

    submit(): void | null {
        return null
    }

    mountForm(form: HTMLFormElement): void {
        this.container.appendChild(this.title)
        this.container.appendChild(this.description)
        this.container.appendChild(this.qrContainer)
        this.container.appendChild(this.instructions)
        this.container.appendChild(this.warningText)

        form.appendChild(this.container)

        // Generate and display QR code
        this.qrImage.src = this.generateQRCodeUrl()
    }
}

/**
 * Prompt for credentials and show QR code.
 * Used when the user wants to get QR code but we need their password.
 */
export class QRAuthCredentialsPrompt extends FormModal<{ username: string, password: string }> {
    private container: HTMLDivElement = document.createElement("div")
    private title: HTMLHeadingElement = document.createElement("h2")
    private description: HTMLParagraphElement = document.createElement("p")
    private passwordInput: HTMLInputElement = document.createElement("input")
    private passwordLabel: HTMLLabelElement = document.createElement("label")

    private username: string

    constructor(username: string) {
        super()
        this.username = username

        this.title.innerText = "Enter Password for QR Code"
        this.title.style.marginBottom = "8px"

        this.description.innerText = `Enter your password to generate a login QR code for user "${username}".`
        this.description.style.color = "#888"
        this.description.style.marginBottom = "16px"

        this.passwordLabel.innerText = "Password"
        this.passwordLabel.style.display = "block"
        this.passwordLabel.style.marginBottom = "4px"
        this.passwordLabel.style.fontSize = "14px"

        this.passwordInput.type = "password"
        this.passwordInput.placeholder = "Enter your password"
        this.passwordInput.style.width = "100%"
        this.passwordInput.style.padding = "8px"
        this.passwordInput.style.marginBottom = "16px"
        this.passwordInput.style.borderRadius = "4px"
        this.passwordInput.style.border = "1px solid #ccc"
        this.passwordInput.required = true
    }

    reset(): void {
        this.passwordInput.value = ""
    }

    submit(): { username: string, password: string } | null {
        const password = this.passwordInput.value
        if (!password) {
            return null
        }
        return { username: this.username, password }
    }

    mountForm(form: HTMLFormElement): void {
        this.container.appendChild(this.title)
        this.container.appendChild(this.description)
        this.container.appendChild(this.passwordLabel)
        this.container.appendChild(this.passwordInput)

        form.appendChild(this.container)

        // Focus password input
        setTimeout(() => this.passwordInput.focus(), 100)
    }
}

