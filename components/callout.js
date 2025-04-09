import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const notificationTp = html`
  <wa-callout>
    <wa-icon slot="icon" hidden></wa-icon>
    <span slot="message"></span>
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

export class NotificationVirto extends HTMLElement {
  static get TAG() { return "virto-notification"; }

  static get observedAttributes() {
    return ["variant", "appearance", "size", "message", "icon"];
  }

  constructor() {
    super();
    const shadow = this.attachShadow({ mode: "open" });
    shadow.append(notificationTp.content.cloneNode(true));
    shadow.adoptedStyleSheets = [globalStyles, notificationCss];

    this.callout = shadow.querySelector("wa-callout");
    this.icon = shadow.querySelector("wa-icon");
    this.message = shadow.querySelector("span[slot='message']");
    this.syncInitialAttributes();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.callout) return;

    if (name === "message") {
      this.message.textContent = newValue || "Notification";
    } else if (name === "icon") {
      if (newValue) {
        this.icon.removeAttribute("hidden");
        this.icon.setAttribute("name", newValue);
        this.icon.setAttribute("variant", "regular"); // Default to regular
      } else {
        this.icon.setAttribute("hidden", "");
      }
    } else {
      if (newValue !== null) {
        this.callout.setAttribute(name, newValue);
      } else {
        this.callout.removeAttribute(name);
      }
    }
  }

  syncInitialAttributes() {
    this.constructor.observedAttributes.forEach((name) => {
      const value = this.getAttribute(name);
      this.attributeChangedCallback(name, null, value);
    });
  }
}

if (!customElements.get(NotificationVirto.TAG)) {
  customElements.define(NotificationVirto.TAG, NotificationVirto);
}