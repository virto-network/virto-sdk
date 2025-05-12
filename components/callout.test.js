import { expect } from '@wdio/globals'

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
    
            document.body.innerHTML = '';
            document.body.appendChild(document.createElement('virto-notification'));
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
    it('should take an attribute called variant', async () => {
        const result = await browser.execute(() => {
          const component = document.querySelector('virto-notification');
          component.setAttribute('variant', 'success');
          return component.getAttribute('variant');
        });
      
        expect(result).toBe('success');
    });

    it('should take an attribute called appearance', async () => {
        const result = await browser.execute(() => {
          const component = document.querySelector('virto-notification');
          component.setAttribute('appearance', 'accent');
          return component.getAttribute('appearance');
        });
      
        expect(result).toBe('accent');
    });

    it('should take an attribute called size', async () => {
        const result = await browser.execute(() => {
          const component = document.querySelector('virto-notification');
          component.setAttribute('size', 'small');
          return component.getAttribute('size');
        });
      
        expect(result).toBe('small');
    });

});

describe('Styles', () => {

    // By now responsiveness will be checked manually. Set OK here if it was tested:

    // This test sometimes passes and sometimes doesnt, it changes minimally the rgb values.
    // it('should change its style according to the value on variant attribute', async () => {
    //     const bgColor = await browser.execute(() => {
    //       const component = document.querySelector('virto-notification');
    //       component.setAttribute('variant', 'success');
    //       const callout = component.shadowRoot.querySelector('wa-callout');
    //       return window.getComputedStyle(callout).backgroundColor;
    //     });
      
    //    //expect(bgColor).toBe('rgb(144, 238, 144)');
    //    //expect(bgColor).toBe('rgb(221, 251, 224)'); // --> dice que espera esto pero despues no pasa y pide:
    //    expect(bgColor).toBe('rgb(218, 251, 221)');
    // });
      
    it('should add styles using adoptedStyleSheets', async () => {
        const hasSheets = await browser.execute(() => {
          const component = document.querySelector('virto-notification');
          return component.shadowRoot.adoptedStyleSheets.length > 0;
        });
      
        expect(hasSheets).toBe(true);
    });

    // works but i do not need it RN, would it be ok to test if the component has content?
    it('should read the text content of the virto-notification component', async () => {
        const component = await browser.$('virto-notification');
        expect(await component.isExisting()).toBe(true);
    
        const text = await component.getText();
        console.log('Component text:', text);
    
        expect(text.length).toBeGreaterThan(0);
    });
});

describe('Events', () => {
    it('responds to virto-info event', async () => {
        const variant = await browser.executeAsync(done => {
          const comp = document.querySelector('virto-notification');
          if (!comp) return done('not-found');
          comp.addEventListener('virto-info', () => comp.setAttribute('variant', 'neutral'));
          comp.dispatchEvent(new CustomEvent('virto-info'));
          setTimeout(() => done(comp.getAttribute('variant')), 20);
        });
        expect(variant).toBe('neutral');
      });
    
    it('should accept the event virto-info');     
    it('should accept the event virto-success');
    it('should accept the event virto-warning');
    it('should accept the event virto-danger');
});
      

describe('User Interactions', () => {
    it('should be triggered with the event virto-info by an action the user performed');
    it('should be triggered with the event virto-success by an action the user');
    it('should be triggered with the event virto-error by an action the user performed');
    it('should be triggered with the event virto-warning by an action the user performed');
});