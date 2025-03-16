import "https://early.webawesome.com/webawesome@3.0.0-alpha.11/dist/components/dialog/dialog.js"
import("https://cdn.jsdelivr.net/npm/virto-components@0.1.7/dist/virto-components.min.js")

import SDK from "https://cdn.jsdelivr.net/npm/@virtonetwork/sdk@0.0.2/dist/esm/sdk.js";

const tagFn = (fn) => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, "").concat(strings[parts.length]))
const html = tagFn((s) => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'));
const css = tagFn((s) => s)
const SRC_URL = new URL(import.meta.url)
const PARAMS = SRC_URL.searchParams
const SERVER_URL = PARAMS.get('server') ?? 'http://localhost:3000'

const dialogTp = html`
    <wa-dialog light-dismiss with-header with-footer>
        <header slot="label">
            <slot name="logo"></slot>
            <slot name="title"></slot>
        </header>
        <hr>
        <div id="content-slot"></div>
        <div id="buttons-slot" name="buttons"></div> 
    </wa-dialog>
`

const dialogCss = css`
:host, wa-dialog {
    font-family: 'Outfit', sans-serif !important;
}

* {
    color: var(--darkslategray) !important;
}

wa-dialog::part(base) {
    padding: 1em;
    background: var(--gradient);
    border-radius: 12px;
    box-shadow: 0px 2px var(--Blurblur-3, 3px) -1px rgba(26, 26, 26, 0.08),
                0px 1px var(--Blurblur-0, 0px) 0px rgba(26, 26, 26, 0.08);
}

#buttons-slot {
    display: flex;
    gap: .5em;
}

hr { 
    border-top: 1px solid var(--lightgreen);
}

[slot="label"] {
    display: flex;
    align-items: center;
    gap: 1em;
}

fieldset {
    border-color: transparent;
    margin-bottom: 1em;
    padding: 0;
}

virto-input:focus {
  outline: none;
}

`

const loginFormTemplate = html`
    <form id="register-form">
        <fieldset>
            <virto-input value="John Doe" label="Name" placeholder="Enter your name" name="name" type="text" required></virto-input>
            <virto-input value="johndoe" label="Username" placeholder="Enter your username" name="username" type="text" required></virto-input>
        </fieldset>
    </form>
`;

const registerFormTemplate = html`
    <form id="login-form">
        <fieldset>
            <virto-input value="johndoe" label="Username" placeholder="Enter your username" name="username" type="text" required></virto-input>
        </fieldset>
    </form>
`;

export class VirtoConnect extends HTMLElement {
  static TAG = "virto-connect"

  constructor() {
    super()
    this.attachShadow({ mode: "open" })
    this.shadowRoot.appendChild(dialogTp.content.cloneNode(true))
    this.sdk = new SDK({
      federate_server: SERVER_URL,
      config: {
        wallet: "polkadotjs"
      }
    });

    const style = document.createElement("style")
    style.textContent = dialogCss
    this.shadowRoot.appendChild(style)

    this.dialog = this.shadowRoot.querySelector("wa-dialog")
    this.contentSlot = this.shadowRoot.querySelector("#content-slot")
    this.buttonsSlot = this.shadowRoot.querySelector("#buttons-slot")
    
    this.currentFormType = "login";
  }

  connectedCallback() {
    this.currentFormType = this.getAttribute("form-type") || "login";
    this.renderCurrentForm();
  }
  
  renderCurrentForm() {
    this.contentSlot.innerHTML = "";
    
    let formTemplate;
    switch (this.currentFormType) {
        case "register":
            formTemplate = registerFormTemplate;
            break;
        case "login":
        default:
            formTemplate = loginFormTemplate;
            break;
    }

    this.contentSlot.appendChild(formTemplate.content.cloneNode(true));
    
    this.updateButtons();
    
    this.updateDialogTitle();
  }
  
  updateDialogTitle() {
    const title = this.currentFormType === "login" ? "Sign Up" : "Sign In";
    const existingTitle = this.querySelector('[slot="title"]');
    if (existingTitle) {
      existingTitle.textContent = title;
    } else {
      const titleElement = document.createElement("h2");
      titleElement.textContent = title;
      titleElement.slot = "title";
      this.appendChild(titleElement);
    }
  }

  updateButtons() {
    this.buttonsSlot.innerHTML = "";
    
    const closeButton = document.createElement("virto-button");
    closeButton.setAttribute("data-dialog", "close");
    closeButton.setAttribute("label", "Close");
    closeButton.addEventListener("click", () => this.close());
    this.buttonsSlot.appendChild(closeButton);
    
    const toggleButton = document.createElement("virto-button");
    const toggleLabel = this.currentFormType === "login" ? "Already have an account? Sign In" : "Need an account? Sign Up";
    toggleButton.setAttribute("label", toggleLabel);
    toggleButton.addEventListener("click", () => {
      this.currentFormType = this.currentFormType === "login" ? "register" : "login";
      this.renderCurrentForm();
    });
    this.buttonsSlot.appendChild(toggleButton);

    const actionButton = document.createElement("virto-button");
    
    if (this.currentFormType === "register") {
      actionButton.setAttribute("label", "Sign In");
      actionButton.addEventListener("click", async () => await this.submitFormLogin());
    } else {
      actionButton.setAttribute("label", "Register");
      actionButton.addEventListener("click", async () => await this.submitFormRegister());
    }
    
    this.buttonsSlot.appendChild(actionButton);
  }

  async submitFormRegister() {
    const form = this.shadowRoot.querySelector("#register-form");
    const formData = new FormData(form);
    const username = formData.get("username");
    
    console.log("Name from FormData:", formData.get("name"));
    console.log("Username from FormData:", username);

    // Check if user is already registered
    try {
      const isRegistered = await this.sdk.auth.isRegistered(username);
      
      if (isRegistered) {
        console.log(`User ${username} is already registered`);
        
        this.buttonsSlot.innerHTML = "";
        
        const errorMsg = document.createElement("div");
        errorMsg.textContent = "This user is already registered. Please sign in instead.";
        errorMsg.style.color = "#d32f2f";
        errorMsg.style.marginBottom = "10px";
        
        const existingErrorMsg = this.contentSlot.querySelector(".error-message");
        if (existingErrorMsg) {
          existingErrorMsg.remove();
        }
        
        errorMsg.className = "error-message";
        this.contentSlot.appendChild(errorMsg);
        
        const cancelButton = document.createElement("virto-button");
        cancelButton.setAttribute("label", "Cancel");
        cancelButton.addEventListener("click", () => this.close());
        this.buttonsSlot.appendChild(cancelButton);

        const loginButton = document.createElement("virto-button");
        loginButton.setAttribute("label", "Continue with Sign In");
        loginButton.addEventListener("click", () => {
          errorMsg.remove();
          this.currentFormType = "register";
          this.renderCurrentForm();
        });
        this.buttonsSlot.appendChild(loginButton);
        
        return;
      }
    } catch (error) {
      console.error('Error checking registration status:', error);
    }

    const user = {
      profile: {
        id: username,
        name: formData.get("name"),
        displayName: username,
      },
      metadata: {},
    };

    try {
      const result = await this.sdk.auth.register(user);
      console.log('Registration successful:', result);
      
      const successMsg = document.createElement("div");
      successMsg.textContent = "Registration successful! You can now sign in.";
      successMsg.style.color = "#4caf50";
      successMsg.style.marginBottom = "10px";
      
      this.contentSlot.innerHTML = "";
      this.contentSlot.appendChild(successMsg);
      
      this.buttonsSlot.innerHTML = "";
      
      const closeBtn = document.createElement("virto-button");
      closeBtn.setAttribute("label", "Close");
      closeBtn.addEventListener("click", () => this.close());
      this.buttonsSlot.appendChild(closeBtn);
      
      const signInBtn = document.createElement("virto-button");
      signInBtn.setAttribute("label", "Sign In Now");
      signInBtn.addEventListener("click", () => {
        this.currentFormType = "register";
        this.renderCurrentForm();
      });
      this.buttonsSlot.appendChild(signInBtn);
      
    } catch (error) {
      console.error('Registration failed:', error);
      
      const errorMsg = document.createElement("div");
      errorMsg.textContent = "Registration failed. Please try again.";
      errorMsg.style.color = "#d32f2f";
      errorMsg.style.marginBottom = "10px";
      this.contentSlot.appendChild(errorMsg);
    }
  }

  async submitFormLogin() {
    const form = this.shadowRoot.querySelector("#login-form");
    const formData = new FormData(form);
    const username = formData.get("username");
    console.log("Username from FormData:", username);

    try {
      const result = await this.sdk.auth.connect(username);
      console.log('Login successful:', result);
      
      const successMsg = document.createElement("div");
      successMsg.textContent = "Login successful!";
      successMsg.style.color = "#4caf50";
      successMsg.style.marginBottom = "10px";
      
      this.contentSlot.innerHTML = "";
      this.contentSlot.appendChild(successMsg);
      
      this.buttonsSlot.innerHTML = "";
      
      const closeBtn = document.createElement("virto-button");
      closeBtn.setAttribute("label", "Close");
      closeBtn.addEventListener("click", () => this.close());
      this.buttonsSlot.appendChild(closeBtn);
      
    } catch (error) {
      console.error('Login failed:', error);
      
      const errorMsg = document.createElement("div");
      errorMsg.textContent = "Login failed. Please check your username and try again.";
      errorMsg.style.color = "#d32f2f";
      errorMsg.style.marginBottom = "10px";
      this.contentSlot.appendChild(errorMsg);
    }
  }

  open() {
    this.dialog.open = true
  }

  close() {
    this.dialog.open = false
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (name === "id" && this.shadowRoot) {
      this.updateDialogTitle();
    } else if (name === "logo") {
      this.updateLogo();
    } else if (name === "form-type" && oldValue !== newValue) {
      this.currentFormType = newValue || "login";
      if (this.shadowRoot) {
        this.renderCurrentForm();
      }
    }
  }

  updateLogo() {
    const logoSlot = this.shadowRoot.querySelector('slot[name="logo"]')
    if (logoSlot) {
      const existingLogo = this.querySelector('[slot="logo"]')
      if (existingLogo) {
        existingLogo.remove()
      }

      const logoSrc = this.getAttribute("logo")
      if (logoSrc) {
        const avatar = document.createElement("virto-avatar")
        avatar.setAttribute("image", logoSrc)
        avatar.setAttribute("slot", "logo")
        this.appendChild(avatar)
      }
    }
  }

  static get observedAttributes() {
    return ["id", "logo", "form-type"]
  }
}

await customElements.whenDefined("wa-dialog")
customElements.define(VirtoConnect.TAG, VirtoConnect)
