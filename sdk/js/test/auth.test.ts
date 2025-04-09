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
      // @ts-ignore
      window.jsWalletFn = (mnemonic: string | null = null) => {
        return {
          unlock: async () => { /* no-op */ },
          getAddress: () => ({
            toHex: () => "0xMOCKADDRESS",
            repr: "0xMOCKREPRESENTATION"
          }),
          sign: (_: any) => "mock-signature",
          phrase: mnemonic || "mock-mnemonic",
          getMnemonic: () => mnemonic || "mock-mnemonic"
        };
      };
    });
  });

  it("should register a user successfully", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      const user = {
        profile: {
          id: "123",
          name: "john.doe",
          displayName: "John Doe",
        },
        metadata: { role: "admin" },
      };

      try {
        const res = await sdk.auth.register(user);
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
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      try {
        const res = await sdk.auth.connect("123");
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
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      await sdk.auth.connect(userId);
    }, userId);

    const result = await page.evaluate(async (userId, command) => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      try {
        const res = await sdk.auth.sign(userId, command);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    }, userId, command);

    if (!result.ok) {
      throw new Error("Signing failed: " + result.error);
    }

    expect(result.data!).toBeDefined();
    expect(result.data!.userId).toBe(userId);
    expect(result.data!.originalExtrinsic).toBe(command.hex);
    expect(result.data!.signedExtrinsic).toBeTruthy();
  });
});
