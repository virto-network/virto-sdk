class DialogoModal extends HTMLElement {
    static css = `
        :host {
            all: initial;
            display: flex;
            flex: 1;
            justify-content: center;
            align-items: center;
            height: 100dvh;
            overflow-x: auto;
        }

        div {
            display: flex;
            flex-direction: column;
            gap: 1em;
            font-family: Outfit, sans-serif;
            width: 100%;
            max-width: 528px;
            height: fit-content;
            background: linear-gradient(0deg, rgba(231, 247, 232, 0.5), rgba(231, 247, 232, 0.5)),
                        radial-gradient(84.04% 109.28% at 10.3% 12.14%, rgba(86, 201, 96, 0.5) 0%, rgba(198, 235, 199, 0) 98.5%);
            border-radius: 12px;
            box-shadow: 0px 2px var(--Blurblur-3, 3px) -1px rgba(26, 26, 26, 0.08),
                        0px 1px var(--Blurblur-0, 0px) 0px rgba(26, 26, 26, 0.08);
            backdrop-filter: blur(32px);
            padding: 1em;
            gap: clamp(4px, 1vw, var(--spacing7, 14px));
            opacity: 1;
        }

        header {
            display: flex;
            align-items: center;
            gap: 1em;
        }

        header h2 {
            font-size: 1.4em;
            font-weight: 600;
            margin: 0;
        }

        hr {
            border: none;
            border-radius: 1px;
            border-top: 1px solid var(--color-accent);
            margin: 1em 0;
        }
    `;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        const style = document.createElement("style");
        const dialog = document.createElement("div");

        style.innerHTML = DialogoModal.css;
        this.username = "";

        dialog.innerHTML = `
            <header>
                <slot name="logo"></slot>
                <h2>${this.getAttribute("label") || "Login to Virto"}</h2>
            </header>
            <hr>
            <slot name="action"></slot>
        `;

        this.shadowRoot.append(style, dialog);
    }
}

customElements.define('dialog-virto', DialogoModal);