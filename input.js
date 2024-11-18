
class Input extends HTMLElement {
    
    static css = `
    :host {
        display: block;
        width: 100%;
    }
    input {
        width: 100%;
        box-sizing: border-box;
        line-height: 28px;
        border-radius: 12px;
        padding: 1em;
        border: 1px solid var(--color-accent);
        &:focus {
            outline: 1px solid var(--color-fill-btn);
        }
    }
`;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        const style = document.createElement("style");
        const input = document.createElement("input");

        style.innerHTML = Input.css;

        this.shadowRoot.append(style, input);
    }
}
  
  customElements.define('input-virto', Input);
