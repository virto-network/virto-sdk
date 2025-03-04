import "https://early.webawesome.com/webawesome@3.0.0-alpha.11/dist/components/dialog/dialog.js"
import("https://cdn.jsdelivr.net/npm/virto-components@0.1.7/dist/virto-components.min.js")

const tagFn = (fn) => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, "").concat(strings[parts.length]))
const html = tagFn((s) => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'));
const css = tagFn((s) => s)

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
    <form id="login-form">
      <fieldset>
        <virto-input label="Username" placeholder="Enter your username" name="username" type="text" required></virto-input>
        <virto-input label="Server" placeholder="Enter server address" name="server" type="text" required></virto-input>
        <virto-input label="Password" placeholder="********" name="password" type="password" required></virto-input>
      </fieldset>
    </form>
`;

export class VirtoConnect extends HTMLElement {
  static TAG = "virto-connect"

  constructor() {
    super()
    this.attachShadow({ mode: "open" })
    this.shadowRoot.appendChild(dialogTp.content.cloneNode(true))

    const style = document.createElement("style")
    style.textContent = dialogCss
    this.shadowRoot.appendChild(style)

    this.dialog = this.shadowRoot.querySelector("wa-dialog")
    this.contentSlot = this.shadowRoot.querySelector("#content-slot")
    this.buttonsSlot = this.shadowRoot.querySelector("#buttons-slot")
  }

  connectedCallback() {
    this.contentSlot.appendChild(loginFormTemplate.content.cloneNode(true));
    this.updateButtons();
  }


  updateButtons() {
    this.buttonsSlot.innerHTML = "";

    const closeButton = document.createElement("virto-button");
    closeButton.setAttribute("data-dialog", "close");
    closeButton.setAttribute("label", "Close");
    closeButton.addEventListener("click", () => this.close());
    this.buttonsSlot.appendChild(closeButton);

    const loginButton = document.createElement("virto-button");
    loginButton.setAttribute("data-dialog", "login");
    loginButton.setAttribute("label", "Log In");
    loginButton.addEventListener("click", () => this.submitForm());
    this.buttonsSlot.appendChild(loginButton);
  }

  submitForm() {
    const form = this.shadowRoot.querySelector("#login-form");
    const formData = new FormData(form);
    console.log("Username from FormData:", formData.get("username"));
    console.log("Server from FormData:", formData.get("server"));
    console.log("Password from FormData:", formData.get("password"));
    const values = Object.fromEntries(formData.entries());
    console.log("Form Data as object:", values);
    this.close();
  }

  open() {
    this.dialog.open = true
  }

  close() {
    this.dialog.open = false
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (name === "id" && this.shadowRoot) {
      const titleSlot = this.shadowRoot.querySelector('slot[name="title"]')
      if (titleSlot) {
        const existingTitle = this.querySelector('[slot="title"]')
        if (existingTitle) {
          existingTitle.remove()
        }
        const titleElement = document.createElement("h2")
        titleElement.textContent = newValue
        titleElement.slot = "title"
        this.appendChild(titleElement)
      }
    } else if (name === "logo") {
      this.updateLogo()
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
    return ["id", "logo"]
  }
}

await customElements.whenDefined("wa-dialog")
customElements.define(VirtoConnect.TAG, VirtoConnect)
