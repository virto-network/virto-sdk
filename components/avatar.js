import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const avatarTp = html`
  <wa-avatar>
    <slot name="icon"></slot>
  </wa-avatar>
`;

const avatarCss = await css`
  :host {
    display: inline-block;
  }

  wa-avatar {
    --background-color: var(--extra-light-green);
    --text-color: var(--darkslategray);
    --size: 48px;
    font-family: var(--font-primary);
    --border-radius: 50px;
    border-radius: var(--border-radius);
    transition: transform 0.2s ease-out, background-color 0.2s ease-out;
  }

  wa-avatar::part(base) {
    transition: all 0.2s ease-out;
  }

  wa-avatar::part(icon) {
    color: var(--green);
  }

  wa-avatar::part(initials) {
    font-weight: 600;
  }

  wa-avatar::part(image) {
    object-fit: cover;
  }

  :host([shape="rounded-square"]) wa-avatar {
    --border-radius: 12px;
    border-radius: var(--border-radius);
  }

  :host(:hover) wa-avatar:hover {
    transform: scale(1.1);
    --background-color: var(--whitish-green);
  }
`;

export class AvatarVirto extends HTMLElement {
  static TAG = "virto-avatar";

  static observedAttributes = ["image", "label", "initials", "loading", "shape"];

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.shadowRoot.appendChild(avatarTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, avatarCss];

    this.waAvatar = this.shadowRoot.querySelector("wa-avatar");
    this.waAvatar.addEventListener("wa-error", this.#handleError);
    this.updateAvatar();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.waAvatar) return;
    this.updateAvatar();
  }

  updateAvatar() {
    const attrs = ["image", "label", "initials", "loading"];
    attrs.forEach(attr => {
      const value = this.getAttribute(attr);
      if (value !== null) this.waAvatar.setAttribute(attr, value);
      else this.waAvatar.removeAttribute(attr);
    });
  }

  disconnectedCallback() {
    this.waAvatar.removeEventListener("wa-error", this.#handleError);
  }

  #handleError = (event) => {
    this.dispatchEvent(
      new CustomEvent("virto-avatar-error", {
        bubbles: true,
        composed: true,
        detail: event.detail || { message: "Image didn’t load", url: this.getAttribute("image") }
      })
    );
  };
}

if (!customElements.get(AvatarVirto.TAG)) {
  customElements.define(AvatarVirto.TAG, AvatarVirto);
}