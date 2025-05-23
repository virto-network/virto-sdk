import { Page } from 'puppeteer';
import { toBase64 } from '../../src/utils/base64';

export class MockServiceRegister {
  private client: import('puppeteer').CDPSession | undefined;
  private credentialId: string | undefined;

  constructor(private page: Page) { }

  getCredentialId() {
    return this.credentialId;
  }

  async setup() {
    await this.page.setRequestInterception(true);
    await this.setupRequestInterceptor();
    await this.setupVirtualAuthenticator();
    this.page.on('console', async msg => {
      for (let arg of msg.args()) {
        console.log(await arg.jsonValue());
      }
    });
    await this.page.goto("http://localhost:3000/fake");
    await new Promise((r) => setTimeout(r, 2000));
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

      if (url.includes("/attestation") && method === "GET") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify(this.getPreRegisterResponse())
        });
        return;
      }

      if (url.includes("/register") && method === "POST") {
        // Parse the JSON body so we can grab the credential ID
        const postData = req.postData() || "{}";
        try {
          const bodyJson = JSON.parse(postData);
          if (bodyJson.attestationResponse.id) {
            this.credentialId = bodyJson.attestationResponse.id;
          }
        } catch (err) {
          // ignore parse errors
        }

        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify({ success: true }),
        });
        return;
      }

      if (url.includes("/assertion") && method === "GET") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify(this.getPreConnectResponse())
        });
        return;
      }

      if (url.includes("/connect") && method === "POST") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify(this.getConnectResponse())
        });
        return;
      }

      if (url.includes("/check-user-registered") && method === "GET") {
        req.respond({
          status: 200,
          headers: this.getCorsHeaders(),
          body: JSON.stringify({ ok: true })
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
          import SDK from '/dist/esm/sdk.js';

          window.SDK = SDK;

          window.WalletType = {
            VIRTO: "virto",
            POLKADOT: "polkadot"
          };
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
      },
      blockNumber: 12345
    };
  }

  private getPreConnectResponse() {
    const challengeBytes = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    const challengeB64 = toBase64(challengeBytes);
    
    let allowCredentials: { id: string; type: string; transports: string[]; }[] = [];
    console.log("credentialId service fixture", this.credentialId);
    if (this.credentialId) {
      allowCredentials = [
        {
          id: this.credentialId,
          type: "public-key",
          transports: ["usb", "internal"],
        },
      ];
    }

    return {
      publicKey: {
        challenge: challengeB64,
        allowCredentials,
        timeout: 60000,
      },
      blockNumber: 12345
    };
  }

  getConnectResponse() {
    return { 
      success: true,
      command: {
        url: "https://kreivo.io/pass/authenticate",
        body: {
          deviceId: "0x01020304",
          credential: {},
          duration: null
        },
        hex: "0x01020304"
      }
    }
  }
}
