class Logo extends HTMLElement {
    static TAG = 'community-logo'
    static css = `
        :host {
            all: initial;
            display: block;
            width: clamp(70px, 5vw, 200px);
            height: auto;
            display: flex;
            justify-content: center;
            align-items: center;
        }

        img {
            max-width: 100%;
            height: auto;
            display: block;
        }
    `;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        const style = document.createElement("style");
        const container = document.createElement("div");

        style.innerHTML = Logo.css;
        container.innerHTML = `
            <img src="${this.getAttribute("src") || "public/virto.svg"}" alt="Logo Virto Network">
        `;

        this.shadowRoot.append(style, container);
    }
}

customElements.define(Logo.TAG, Logo);
