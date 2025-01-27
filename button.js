class ButtonVirto extends HTMLElement {
    static TAG = 'virto-button'
    static css = `
    :host {
        display: inline-block;
        width: 100%;
    }
    button {
        font-family: Outfit, sans-serif;
        cursor: pointer;
        width: 100%;
        height: 44px;
        min-height: 44px;
        padding: 12px;
        border-radius: 1000px;
        border: 1px solid #1A1A1A1F;
        opacity: 1;
        background-color: var(--color-fill-btn);
        color: var(--color-bg);
        transition: background-color 500ms ease;
    }
    button:hover {
        background-color: var(--color-accent);
    }
    :host([variant="secondary"]) button {
        background-color: var(--color-input);
        color: var(--color-txt);
        border: 1px solid var(--color-alt-rgb);
    }
    :host([variant="secondary"]) button:hover,
    :host([variant="secondary"]) button:focus {
        background-color: var(--color-fill-social-btn);
    }
    `;

    constructor() {
      super();
      this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
      this.render();
    }
    
    render() {
      const style = document.createElement("style");
      const btn = document.createElement("button");

      style.textContent = ButtonVirto.css;
      btn.textContent = this.getAttribute("label") || "Button";
      this.shadowRoot.innerHTML = '';
      this.shadowRoot.append(style, btn);
    }

    static get observedAttributes() {
        return ['label', 'variant'];
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (name === 'label' && this.shadowRoot) {
            const btn = this.shadowRoot.querySelector('button');
            if (btn) {
                btn.textContent = newValue || "Button";
            }
        }
        if (name === 'variant') {
            this.render();
        }
    }
}
  
customElements.define(ButtonVirto.TAG, ButtonVirto);