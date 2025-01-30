import { toBase64 } from '../src/utils/base64';

describe("Auth - E2E WebAuthn Flow", () => {
  let client: import('puppeteer').CDPSession;
  let authenticatorId: string;

  beforeAll(async () => {
    await page.setRequestInterception(true);

    page.on("request", async (req) => {
      const url = req.url();
      const method = req.method();

      if (url.endsWith("/pre-register") && method === "POST") {
        const challengeBytes = new Uint8Array([
          1, 2, 3, 4, 5, 6, 7, 8,
          9, 10, 11, 12, 13, 14, 15, 16
        ]);
        const challengeB64 = toBase64(challengeBytes);

        const userIdBytes = new Uint8Array([9, 9, 9, 9, 9, 9, 9, 9]);
        const userIdB64 = toBase64(userIdBytes);

        req.respond({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
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
          }),
        });
        return;
      }

      if (url.endsWith("/post-register") && method === "POST") {
        req.respond({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ success: true }),
        });
        return;
      }

      req.continue();
    });


    client = await page.target().createCDPSession();

    await client.send("WebAuthn.enable");

    const result = await client.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "usb",
        hasResidentKey: false,
        hasUserVerification: false,
        isUserVerified: false
      }
    });
    authenticatorId = result.authenticatorId;
    console.log("Authenticator ID:", authenticatorId);

    await page.goto("http://localhost:3000/index.html");
  });

  it("should register a user successfully (happy path)", async () => {

    const result = await page.evaluate(async () => {

      const auth = new window.Auth("http://localhost:3000");
      const user = {
        profile: {
          id: "123",
          name: "john.doe",
          displayName: "John Doe",
        },
        metadata: { role: "admin" },
      };

      try {
        const res = await auth.register(user);
        return { ok: true, data: res };
      } catch (err) {
        const e = err as Error;
        return { ok: false, error: e.message || String(e) };
      }
    });

    if (!result.ok) {
      console.log(result);
      throw new Error("Registration failed: " + result.error);
    }
    expect(result.data).toBeDefined();
    expect(result.data.success).toBe(true);
  });
});
