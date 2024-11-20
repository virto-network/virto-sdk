

class ButtonVirto extends HTMLElement {
    static TAG = 'button-virto'
    static css = `
    button {
        font-family: Outfit, sans-serif;
        cursor: pointer;
        width: 100%;
        height: 44px;
        min-height: 44px;
        padding: 12px;
        border-radius: 1000px;
        border: 1px solid var(--colors-alpha-dark-100, #1A1A1A1F)
        opacity: 0px;
        background-color: var(--color-fill-btn);
        color: var(--color-bg);
        transition: background-color 500ms ease;
        &:hover, &:focus {
            background-color: var(--color-accent);
        }
    }
   
`;

    constructor() {
      super();
      this.attachShadow({ mode: 'open' });
      const style = document.createElement("style");
      const btn = document.createElement("button");

      style.innerHTML = ButtonVirto.css;
      btn.innerText = this.getAttribute("label") || "Button";
      this.shadowRoot.append(style, btn);
    }
    
    static get observedAttributes() {
        return ['label'];
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (name === 'label' && this.shadowRoot) {
            const btn = this.shadowRoot.querySelector('button');
            if (btn) {
                btn.innerText = newValue || "Button";
            }
        }
    }
  }
  
  customElements.define(ButtonVirto.TAG, ButtonVirto);