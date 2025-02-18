import { MockServiceRegister } from './fixtures/service.fixture';

describe("Auth - E2E WebAuthn Flow", () => {
  let mockService: MockServiceRegister;

  beforeAll(async () => {
    await page.exposeFunction('mockSube', async (_url: string, options?: any) => {
      return { success: true, extrinsic: options?.body };
    });

    mockService = new MockServiceRegister(page);
    await mockService.setup();

    await page.evaluate(() => {
      window.jsWalletFn = (mnemonic: string | null = null) => {
        return {
          unlock: async () => { /* no-op */ },
          getAddress: () => ({
            toHex: () => "0xMOCKADDRESS",
            repr: "0xMOCKREPRESENTATION"
          }),
          sign: (_: any) => "mock-signature",
          phrase: mnemonic || "mock-mnemonic"
        };
      };
    });
  });

  it("should register a user successfully", async () => {
    const result = await page.evaluate(async () => {
      const subeFn = window.mockSube;
      const jsWalletFn = window.jsWalletFn;

      const auth = new window.Auth("http://localhost:3000", subeFn, jsWalletFn);

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
        return { ok: false, error: (err as Error).message };
      }
    });

    if (!result.ok) {
      throw new Error("Registration failed: " + result.error);
    }
    expect(result.data).toBeDefined();
    expect(result.data.success).toBe(true);
  });

  it("should connect a user successfully", async () => {
    const credentialId = mockService.getCredentialId();
    console.log("credentialId", credentialId);
    const result = await page.evaluate(async (_credentialId) => {
      const subeFn = window.mockSube;
      const jsWalletFn = window.jsWalletFn;

      const auth = new window.Auth("http://localhost:3000", subeFn, jsWalletFn);

      try {
        const res = await auth.connect("123");
        return {
          ok: true,
          data: res,
          storedKeyring: localStorage.getItem('keyring_pair')
        };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    }, credentialId);
    if (!result.ok) {
      throw new Error("Connection failed: " + result.error);
    }
    expect(result.data).toBeDefined();
    expect(result.data.success).toBe(true);
  });

  it("should sign an extrinsic successfully", async () => {
    const credentialId = mockService.getCredentialId();
    console.log("credentialId", credentialId);
    const userId = "123";
    const command = {
      url: "https://kreivo.io/pass/authenticate",
      body: {
        "method": "system",
        "args": {
          "remark": "0x01020304",
        }
      },
      hex: "0x01020304"
    };

    // First, ensure a session is created for the email
    await page.evaluate(async (userId) => {
      const subeFn = window.mockSube;
      const jsWalletFn = window.jsWalletFn;

      const auth = new window.Auth("http://localhost:3000", subeFn, jsWalletFn);
      await auth.connect(userId);
    }, userId);

    const result = await page.evaluate(async (userId, command) => {
      const subeFn = window.mockSube;
      const jsWalletFn = window.jsWalletFn;
      
      const auth = new window.Auth("http://localhost:3000", subeFn, jsWalletFn);

      try {
        const res = await auth.sign(userId, command);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    }, userId, command);

    if (!result.ok) {
      throw new Error("Signing failed: " + result.error);
    }

    expect(result.data).toBeDefined();
    expect(result.data.userId).toBe(userId);
    expect(result.data.originalExtrinsic).toBe(command.hex);
    expect(result.data.signedExtrinsic).toBeTruthy();
  });
});
