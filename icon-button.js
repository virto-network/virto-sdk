class BotonSocial extends HTMLElement {
    static css = `
        :host {
            all: initial;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: clamp(40px, 5vw, 50px);
            height: clamp(40px, 5vw, 50px);
            border-radius: 50%;
            background-color: #C6EBC7;
            box-shadow: 0px 2px 3px rgba(26, 26, 26, 0.08),
                        0px 1px 0px rgba(26, 26, 26, 0.08);
            cursor: pointer;
            transition: transform 0.2s ease, box-shadow 0.2s ease;
            padding: .3em;
        }

        :host(:hover) {
            transform: scale(1.05);
            box-shadow: 0px 4px 6px rgba(26, 26, 26, 0.16),
                        0px 2px 1px rgba(26, 26, 26, 0.1);
        }

        div {
            display: flex;
            align-items: center;
            justify-content: center;
            width: 100%;
            height: 100%;
        }

        img {
            max-width: 70%;
            max-height: 70%;
            display: block;
        }
    `;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        const style = document.createElement("style");
        const container = document.createElement("div");

        style.innerHTML = BotonSocial.css;
        container.innerHTML = `
            <img src="${this.getAttribute("src") || 'public/google.svg'}" alt="${this.getAttribute("alt") || "Social Button"}">
        `;

        this.shadowRoot.append(style, container);
    }
}

customElements.define('button-social', BotonSocial);
