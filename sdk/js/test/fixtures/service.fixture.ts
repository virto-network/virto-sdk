import { Page } from 'puppeteer';
import { toBase64 } from '../../src/utils/base64';

export class MockServiceRegister {
  private client: import('puppeteer').CDPSession | undefined;

  constructor(private page: Page) {}

  async setup() {
    await this.page.setRequestInterception(true);
    await this.setupRequestInterceptor();
    await this.setupVirtualAuthenticator();
    await this.page.goto("http://localhost:3000/fake");
  }

  private async setupRequestInterceptor() {
    this.page.on("request", (req) => {
      const url = req.url();
      const method = req.method();

      if (url === "http://localhost:3000/fake" && method === "GET") {
        req.respond({
          status: 200,
          contentType: "text/html",
          body: this.getInlineHtml()
        });
        return;
      }

      if (url.endsWith("/pre-register") && method === "POST") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify(this.getPreRegisterResponse())
        });
        return;
      }

      if (url.endsWith("/post-register") && method === "POST") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify({ success: true })
        });
        return;
      }

      req.continue();
    });
  }

  private async setupVirtualAuthenticator() {
    this.client = await this.page.target().createCDPSession();
    await this.client.send("WebAuthn.enable");
    await this.client.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "usb",
        hasResidentKey: false,
        hasUserVerification: false,
        isUserVerified: false
      }
    });
  }

  private getInlineHtml(): string {
    return `
      <!DOCTYPE html>
      <html>
      <head><meta charset="utf-8" /><title>Auth Register Test</title></head>
      <body>
        <script type="module">
          import Auth from '/dist/esm/auth.js';
          window.Auth = Auth;
        </script>
      </body>
      </html>
    `;
  }

  private getCorsHeaders() {
    return {
      "Content-Type": "application/json",
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Methods": "POST, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type"
    };
  }

  private getPreRegisterResponse() {
    const challengeBytes = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    const challengeB64 = toBase64(challengeBytes);

    const userIdBytes = new Uint8Array([9, 9, 9, 9, 9, 9, 9, 9]);
    const userIdB64 = toBase64(userIdBytes);

    return {
      publicKey: {
        challenge: challengeB64,
        rp: { name: "Mock RP" },
        user: {
          id: userIdB64,
          name: "john.doe",
          displayName: "John Doe"
        },
        pubKeyCredParams: [{ type: "public-key", alg: -7 }],
        authenticatorSelection: { userVerification: "preferred" },
        timeout: 60000,
        attestation: "none"
      }
    };
  }
}
