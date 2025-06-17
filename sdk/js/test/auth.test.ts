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

  it("should register a user successfully with pepper", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore - SDK with pepper configuration
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO,
          pepper: {
            type: "code", // PepperType.CODE
          }
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      const user = {
        profile: {
          id: "pepper-user-456",
          name: "jane.pepper",
          displayName: "Jane Pepper",
        },
        metadata: { role: "user" },
      };

      const pepperData = {
        type: "code", // PepperType.CODE
        value: "1234"
      };

      try {
        const res = await sdk.auth.register(user, pepperData);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    });

    if (!result.ok) {
      throw new Error("Registration with pepper failed: " + result.error);
    }
    expect(result.data).toBeDefined();
    expect(result.data.success).toBe(true);
  });

  it("should register successfully when pepper is not configured", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore - SDK without pepper configuration
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO
          // No pepper configuration = pepper not required
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      const user = {
        profile: {
          id: "pepper-user-789",
          name: "no.pepper",
          displayName: "No Pepper User",
        },
        metadata: { role: "user" },
      };

      try {
        // Register without providing pepper (should succeed because pepper is not configured)
        const res = await sdk.auth.register(user);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    });

    // Should succeed because pepper is not configured
    expect(result.ok).toBe(true);
    expect(result.data).toBeDefined();
    expect(result.data.success).toBe(true);
  });

  it("should fail to register with invalid pepper", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore - SDK with pepper configuration
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO,
          pepper: {
            type: "code",
          }
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      const user = {
        profile: {
          id: "pepper-user-invalid",
          name: "invalid.pepper",
          displayName: "Invalid Pepper User",
        },
        metadata: { role: "user" },
      };

      const invalidPepperData = {
        type: "code",
        value: "abc" // Invalid: contains letters instead of numbers
      };

      try {
        const res = await sdk.auth.register(user, invalidPepperData);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    });

    expect(result.ok).toBe(false);
    expect(result.error).toContain("Invalid pepper value");
  });

  it("should get pepper info correctly", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore - SDK with pepper configuration
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO,
          pepper: {
            type: "code",
          }
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      try {
        const pepperInfo = sdk.auth.getPepperInfo();
        return { ok: true, data: pepperInfo };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    });

    expect(result.ok).toBe(true);
    expect(result.data.required).toBe(true); // Required because pepper is configured
    expect(result.data.type).toBe("code");
    expect(result.data.supportedTypes).toContain("code");
  });

  it("should fail to register when pepper is configured but not provided", async () => {
    const result = await page.evaluate(async () => {
      // @ts-ignore
      const subeFn = window.mockSube;
      // @ts-ignore
      const jsWalletFn = window.jsWalletFn;
      
      // @ts-ignore - SDK with pepper configuration (required because configured)
      const sdk = new window.SDK({
        federate_server: "http://localhost:3000",
        config: {
          wallet: window.WalletType.VIRTO,
          pepper: {
            type: "code" // PepperType.CODE
          }
        }
      }, subeFn, (mnemonic: string | null) => jsWalletFn(mnemonic || undefined));

      const user = {
        profile: {
          id: "pepper-user-required",
          name: "required.pepper",
          displayName: "Required Pepper User",
        },
        metadata: { role: "user" },
      };

      try {
        // Try to register without providing pepper (should fail)
        const res = await sdk.auth.register(user);
        return { ok: true, data: res };
      } catch (err) {
        return { ok: false, error: (err as Error).message };
      }
    });

    expect(result.ok).toBe(false);
    expect(result.error).toContain("Pepper is required for registration");
  });
});
