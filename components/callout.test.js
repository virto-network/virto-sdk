import { $, expect } from '@wdio/globals'

// This tests should be run on the /components folder with the command: npm run wdio or npm run wdio -- --watch, right after running npm install

describe('Rendering', () => {

    // We ensure a component instance is added before each test
    beforeEach(async () => {
        await browser.execute(async () => {
            if (!window.customElements.get('virto-notification')) {
                const script = document.createElement('script');
                script.type = 'module';
                script.src = './callout.js';
                document.head.appendChild(script);
                await new Promise((resolve) => script.onload = resolve);
            }
    
            const component = document.createElement('virto-notification');
            document.body.innerHTML = '';
            document.body.appendChild(component);
        });
    });
    
    
    it('should render the notification component', async () => {
        const tagName = await browser.execute(() => {
            const component = document.querySelector('virto-notification');
            return component?.tagName.toLowerCase();
        });
    
        expect(tagName).toBe('virto-notification');
    });
    

    it('should create a Shadow DOM root', async () => {
        const hasShadowRoot = await browser.execute(() => {
            const component = document.querySelector('virto-notification');
            return !!component.shadowRoot;
        });
    
        expect(hasShadowRoot).toBe(true);
    });

    it('Shadow DOM should contain the <wa-callout> element', async () => {
        const tagName = await browser.execute(() => {
            const component = document.querySelector('virto-notification');
            const callout = component.shadowRoot?.querySelector('wa-callout');
            return callout?.tagName.toLowerCase() || null;
        });
    
        expect(tagName).toBe('wa-callout');
    });

    it('should test if the virto-notification component was defined', async () => {
        const isDefined = await browser.execute(() => {
            return window.customElements.get('virto-notification') !== undefined;
        });
        expect(isDefined).toBe(true);        
    });

    it('the component tag should start with virto-', async () => {
        const tagName = await browser.execute(() => {
            const component = document.querySelector('virto-notification');
            return component?.tagName.toLowerCase();
        });

        expect(tagName.startsWith('virto-')).toBe(true);
    });

    it('should be able to take content using innerHTML', async () => {
        const innerContent = await browser.execute(() => {
            const component = document.querySelector('virto-notification');
            component.innerHTML = 'Warning!';
            return component.innerHTML;
        });
    
        expect(innerContent).toContain('Warning!');
    });
    
});

describe('Attributes', () => {
    it('should take an attribute called variant');
});

describe('Styles', () => {
    it('should change its style according to the value on variant attribute');
    it('should be responsive');
});

describe('Events', () => {
    it('Should accept the events virto-info, virto-success, virto-error and virto-warning');
});

describe('User Interactions', () => {
    it('should be triggered with the event virto-info by an action the user performed');
    it('should be triggered with the event virto-success by an action the user');
    it('should be triggered with the event virto-error by an action the user performed');
    it('should be triggered with the event virto-warning by an action the user performed');
});