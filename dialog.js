class DialogoModal extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        this.currentStep = 1;
        this.isClosing = false;
        this.isLogin = false;    
    }

    connectedCallback() {
        this.render();
        this.setupEventListeners();
    }

    get totalSteps() {
        return this.querySelectorAll(`[slot^="${this.isLogin ? 'login-' : ''}step-"]`).length;
    }

    render() {
        this.shadowRoot.innerHTML = `
        <style>
            :host {
                all: initial;
                display: none;
                position: fixed;
                top: 0;
                left: 0;
                width: 100%;
                height: 100%;
                background-color: #0000004D;
                justify-content: center;
                align-items: center;
                opacity: 0;
                transition: opacity 0.3s ease;
            }

            :host(.visible) {
                display: flex;
                opacity: 1;
            }

            .dialog {
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
                transform: translateX(100%);
                opacity: 0;
                pointer-events: none;
                transition: transform 0.3s ease, opacity 0.2s ease;
            }
            
            :host(.visible) .dialog {
                transform: translateX(0);
                opacity: 1;
                pointer-events: auto;
                animation: slideInRight 0.5s forwards;
            }
            
            @keyframes slideInRight {
                from {
                    transform: translateX(100%);
                    opacity: 0;
                }
                to {
                    transform: translateX(0);
                    opacity: 1;
                }
            }
            
            @keyframes slideOutLeft {
                from {
                    transform: translateX(0);
                    opacity: 1;
                }
                50% {
                    opacity: 0;
                }
                to {
                    transform: translateX(-100%);
                    opacity: 0;
                }
            }

            header {
                display: flex;
                align-items: center;
                gap: 1em;
            }

            header h2 {
                font-size: 1.4em;
                font-weight: 600;
                color: var(--color-txt);
                margin: 0;
            }

            hr {
                border: none;
                border-radius: 1px;
                border-top: 1px solid var(--color-accent);
                margin: 1em 0;
            }

            .navigation {
                box-sizing: border-box;
                display: flex;
                justify-content: space-between;
                gap: 10px;
                width: 100%;
                flex-direction: row;
                color: var(--color-txt);
            }

            .navigation button-virto {
                flex: 1;
            }

            .step-content {
                display: none;
                opacity: 0;
                transition: opacity 0.3s ease;
            }
            
            .step-content.active {
                display: block;
                opacity: 1;
            }

            .image-step-1 {
                position: absolute;
                top: 0;
                right: 0;
                width: 200px;
                height: 200px;
                object-fit: contain;
                margin: auto 0;
            }

            .center-image {
                display: flex;
                justify-content: center;
                align-items: center;
                width: 200px;
                height: 200px;
                margin: 0 auto;
            }

            .qr-code {
                background-color: #fff;
            }

            .loader {
                width: 70px;
                height: 70px;
                border-radius: 50%;
                animation: spin 1s linear infinite;
                background: conic-gradient(from 0deg at 50% 50%, #F0FDF1 0%, rgba(255, 255, 255, 0) 100%);
                mask: radial-gradient(circle, transparent 60%, black 61%);
                -webkit-mask: radial-gradient(circle, transparent 60%, black 61%);
            }

            @keyframes spin {
                from { transform: rotate(0deg); }
                to { transform: rotate(360deg); }
            }

            .verificated {
                background-color: transparent;
            }
        </style>
        <div class="dialog">
            <header>
                <slot name="logo"></slot>
                <h2 id="step-title"></h2>
            </header>
            <hr>
            ${Array.from({ length: this.totalSteps }, (_, i) => i + 1).map(step => `
                <div class="step-content ${step === this.currentStep ? 'active' : ''}" data-step="${step}">
                    <div class="image-container"></div>
                    <slot name="${this.isLogin ? 'login-' : ''}step-${step}"></slot>
                </div>
            `).join('')}
            <div class="navigation">
                <button-virto id="prevButton" variant="secondary"></button-virto>
                <button-virto id="nextButton"></button-virto>
            </div>
        </div>
        `;
    this.dialog = this.shadowRoot.querySelector('.dialog');
}

    setupEventListeners() {
        this.shadowRoot.getElementById('prevButton').addEventListener('click', (e) => this.handleButtonClick(e, 'prev'));
        this.shadowRoot.getElementById('nextButton').addEventListener('click', (e) => this.handleButtonClick(e, 'next'));
    }

    handleButtonClick(event, direction) {
        const buttonLabel = event.target.getAttribute('label').toLowerCase();

        if (buttonLabel === 'change number') {
            if (!this.isClosing) {
                this.currentStep = 1;
                this.updateStepContent();
            }
            return;
        }

        if (buttonLabel === 'cancel' || buttonLabel === 'close') {
            this.hide();
        } else {
            this.navigate(direction);
        }
    }

    navigate(direction) {
        const stepMapping = this.isLogin ? [1, 2, 3, 5, 6] : [1, 2, 3, 4, 5, 6];
        const currentIndex = stepMapping.indexOf(this.currentStep);
        const nextIndex = direction === 'next' ? currentIndex + 1 : currentIndex - 1;

        if (nextIndex < 0 || nextIndex >= stepMapping.length) {
            return;
        }

        const nextStep = stepMapping[nextIndex];
        this.animateStepTransition(direction, () => {
            this.currentStep = nextStep;
            this.updateStepContent();
        });
    }


    animateStepTransition(direction, callback) {
        const currentContent = this.shadowRoot.querySelector(`.step-content[data-step="${this.currentStep}"]`);
        const nextStep = direction === 'next' ? this.currentStep + 1 : this.currentStep - 1;
        const nextContent = this.shadowRoot.querySelector(`.step-content[data-step="${nextStep}"]`);


        const animation = direction === 'next' ? 'slideOutLeft' : 'slideInRight';
        this.dialog.style.animation = `${animation} 0.3s forwards`;
        
        this.dialog.addEventListener('animationend', () => {
            this.dialog.style.animation = '';
            callback();
        }, { once: true });
    }

    updateStepContent() {
        const stepData = this.steps[this.currentStep - 1];
        this.shadowRoot.getElementById('step-title').textContent = stepData.title;
        
        const prevButton = this.shadowRoot.getElementById('prevButton');
        const nextButton = this.shadowRoot.getElementById('nextButton');

        if (stepData.singleButton) {
            prevButton.style.display = 'none';
            nextButton.setAttribute('label', stepData.singleButtonLabel);

        } else {
            prevButton.style.display = '';
            prevButton.setAttribute('label', stepData.prevButtonLabel);
            nextButton.setAttribute('label', stepData.nextButtonLabel);
        }

        this.shadowRoot.querySelectorAll('.step-content').forEach(el => el.classList.remove('active'));
        this.shadowRoot.querySelector(`.step-content[data-step="${this.currentStep}"]`).classList.add('active');

        this.updateStepImage();

    }

    updateStepImage() {
        const imageContainer = this.shadowRoot.querySelector('.step-content.active .image-container');
        imageContainer.innerHTML = '';

        switch (this.currentStep) {
            case 1:
                imageContainer.innerHTML = '<img class="image-step-1" src="public/Welcome.svg" alt="Illustration">';
                break;
            case 5:
                imageContainer.innerHTML = '<div class="center-image loader"></div>';
                break;
            case 6:
                imageContainer.innerHTML = '<img class="center-image verificated" src="public/verificated.svg" alt="Authentication Successful">';
                break;
            default:
                break;
        }
    }

    show() {
        this.isClosing = false;
        this.classList.add('visible');
        this.currentStep = 1;
        this.updateStepContent();
    }

    hide() {
        if (this.isClosing) return;
        this.isClosing = true;
        this.dialog.style.animation = 'slideOutLeft 0.5s forwards';
        this.dialog.addEventListener('animationend', () => {
            this.classList.remove('visible');
            this.dispatchEvent(new CustomEvent('dialog-closed'));
            this.reset();
            this.isClosing = false;
        }, { once: true });
    }

    reset() {
        this.currentStep = 1;
        this.render();
        this.setupEventListeners();
    }

    get steps() {
        if (this.isLogin) {
            return [
                { title: "Login to Virto", singleButton: true, singleButtonLabel: "Continue" },
                { title: "Login as {username}", singleButton: true, singleButtonLabel: "Continue" },
                { title: "Secure your account", singleButton: true, singleButtonLabel: "Continue" },
                { title: "Secure your account", singleButton: true, singleButtonLabel: "Continue" },
                { title: "Secure your account", singleButton: true, singleButtonLabel: "Close" },
            ];
        } else {
            return [
                { title: "Virto requires you to signup", prevButtonLabel: "Cancel", nextButtonLabel: "Request Code" },
                { title: "Virto requires you to signup", prevButtonLabel: "Change Number", nextButtonLabel: "Continue" },
                { title: "Virto requires you to signup", prevButtonLabel: "Cancel", nextButtonLabel: "Continue" },
                { title: "Secure your account", prevButtonLabel: "Cancel", nextButtonLabel: "Continue" },
                { title: "Secure your account", singleButton: true, singleButtonLabel: "Continue" },
                { title: "Secure your account", singleButton: true, singleButtonLabel: "Close" },
            ];
        }
    }
    setDialogType(type) {
        this.dialogType = type;
        this.isLogin = type === 'login';
        this.currentStep = 1;
        this.updateStepContent();
      }

    setMode(isLogin) {
        this.isLogin = isLogin;
        this.currentStep = 1;
        this.render();
        this.setupEventListeners();
        this.updateStepContent();
    }
}

customElements.define('dialog-virto', DialogoModal);