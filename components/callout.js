import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const notificationTp = html`
  <wa-callout>
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

export class NotificationVirto extends HTMLElement {
  static TAG = "virto-notification";
  // Aparenlty there's no need to add the attributes on the array observedAttributes, unless we want to do something with them on the code. Ive deleted them by now in order to check that the tests pass anyway.

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.shadowRoot.appendChild(notificationTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, notificationCss];

    this.callout = this.shadowRoot.querySelector("wa-callout");
  }

  connectedCallback() {
    // this.addEventListener('virto-info', () => {
    //   this.setAttribute('variant', 'neutral');
    // });
  }
  
}

if (!customElements.get(NotificationVirto.TAG)) {
  customElements.define(NotificationVirto.TAG, NotificationVirto);
}