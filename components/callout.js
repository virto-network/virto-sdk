import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const notificationTp = html`
  <wa-callout>
    <wa-icon slot="icon" hidden></wa-icon>
    <slot></slot>
  </wa-callout>
`;

const notificationCss = await css`
  :host {
    display: block;
    margin: 1em 0;
  }

  wa-callout {
    font-family: Outfit, sans-serif;
    padding: var(--spacing, 1em);
    border-radius: 8px;
    background-color: var(--extra-light-green);
    color: var(--darkslategray);
    transition: all 300ms ease;
  }

  /* Variant Styles */
  :host([variant="brand"]) wa-callout {
    background-color: var(--green);
    color: var(--whitesmoke);
  }

  :host([variant="success"]) wa-callout {
    background-color: var(--lightgreen);
    color: var(--darkslategray);
  }

  :host([variant="warning"]) wa-callout {
    background-color: #ffcc00; /* Example warning color */
    color: var(--darkslategray);
  }

  :host([variant="danger"]) wa-callout {
    background-color: #ff3333; /* Example danger color */
    color: var(--whitesmoke);
  }

  :host([variant="neutral"]) wa-callout {
    background-color: var(--grey-green);
    color: var(--darkslategray);
  }

  /* Appearance Adjustments */
  :host([appearance="plain"]) wa-callout {
    border: none;
  }

  :host([appearance="accent"]) wa-callout {
    border: 2px solid var(--green);
  }

  /* Size Adjustments */
  :host([size="small"]) wa-callout {
    padding: 0.5em;
    font-size: 0.875em;
  }

  :host([size="large"]) wa-callout {
    padding: 1.5em;
    font-size: 1.25em;
  }

  /* Icon Styling */
  wa-icon {
    --icon-size: 1.5em;
    --icon-color: inherit;
  }

  wa-callout:hover {
    opacity: 0.9;
  }
`;

const variantToIcon = {
    'brand': 'circle-info',
    'neutral': 'gear',
    'success': 'circle-check',
    'warning': 'triangle-exclamation',
    'danger': 'circle-exclamation',
    'inherit': 'circle-info'
  };

export class NotificationVirto extends HTMLElement {
  static TAG = "virto-notification";
  static observedAttributes = ["variant", "appearance", "size", "message", "icon"];

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.shadowRoot.appendChild(notificationTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, notificationCss];

    this.callout = this.shadowRoot.querySelector("wa-callout");
    this.icon = this.shadowRoot.querySelector("wa-icon");
    //this.callout.textContent = this.getAttribute("message") || "Notification";
    this.#syncAttributes();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.callout) return;

    if (name === "icon" || name === "variant") {
      this.updateIcon();
    } else {
      if (newValue !== null) {
        this.callout.setAttribute(name, newValue);
      } else {
        this.callout.removeAttribute(name);
      }
    }
  }

  updateIcon() {
    const iconAttr = this.getAttribute("icon");
    const variant = this.getAttribute("variant") || 'inherit';
    console.log(`Updating icon: variant=${variant}, iconAttr=${iconAttr}`);
    if (iconAttr) {
      console.log(`Setting icon to ${iconAttr}`);
      this.icon.setAttribute("name", iconAttr);
      this.icon.setAttribute("variant", "regular");
      this.icon.removeAttribute("hidden");
    } else {
      const defaultIcon = variantToIcon[variant];
      console.log(`Setting default icon to ${defaultIcon}`);
      if (defaultIcon) {
        this.icon.setAttribute("name", defaultIcon);
        this.icon.setAttribute("variant", "regular");
        this.icon.removeAttribute("hidden");
      } else {
        console.log("No icon set, hiding icon");
        this.icon.setAttribute("hidden", "");
      }
    }
  }

  #syncAttributes() {
    const attrs = ["variant", "appearance", "size", "message", "icon"];
    attrs.forEach((name) => {
      const value = this.getAttribute(name);
      this.attributeChangedCallback(name, null, value);
    });
  }
}

if (!customElements.get(NotificationVirto.TAG)) {
  customElements.define(NotificationVirto.TAG, NotificationVirto);
}