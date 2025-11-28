import { BaseProfile, AttestationData, User, SignFn, TransactionResult } from "./types";
import { Blake2256 } from "@polkadot-api/substrate-bindings";
import { mergeUint8 } from "polkadot-api/utils";
import { kreivo, MultiAddress } from "@virtonetwork/sdk/descriptors";
import { Binary, PolkadotClient, PolkadotSigner } from "polkadot-api";
import {
  CredentialsHandler,
  WebAuthn as PasskeysAuthenticator,
} from "@virtonetwork/authenticators-webauthn";
import { KreivoPassSigner } from "@virtonetwork/signer";
import { ss58Encode } from "@polkadot-labs/hdkd-helpers";
import { VOSCredentialsHandler } from "./vocCredentialHandler";
import NonceManager from "./nonceManager";

let base64Module: typeof import("./utils/base64.browser") | null = null;

async function loadBase64Module() {
  if (!base64Module && typeof window !== "undefined") {
    base64Module = await import("./utils/base64.browser");
  }
  return base64Module;
}

export default class Auth {
  private _client: PolkadotClient | null = null;
  private _passkeysAuthenticator: PasskeysAuthenticator | null = null;
  private _sessionSigner: PolkadotSigner & {
    sign: SignFn;
  } | null = null;
  private _currentUserId: string | null = null;
  private _nonceManager: NonceManager | null = null;

  constructor(
    private readonly baseUrl: string,
    private readonly credentialsHandler: CredentialsHandler,
    private readonly clientFactory: () => Promise<PolkadotClient>,
    nonceManager?: NonceManager,
  ) {
    this._nonceManager = nonceManager || null;
    // Preload the module if we're in a browser environment
    if (typeof window !== "undefined") {
      loadBase64Module();
    }
  }

  /**
   * Set the NonceManager instance for this Auth instance
   * This allows the Auth to increment nonces after successful connections
   * 
   * @param nonceManager - The NonceManager instance to use
   */
  setNonceManager(nonceManager: NonceManager): void {
    this._nonceManager = nonceManager;
  }

  private async getClient(): Promise<PolkadotClient> {
    if (!this._client) {
      this._client = await this.clientFactory();
    }
    return this._client;
  }

  async register(user: User<BaseProfile>) {
    const preparedData = await this.prepareRegistration(user);
    const result = await this.completeRegistration(
      preparedData.attestation,
      preparedData.hashedUserId,
      preparedData.credentialId,
      preparedData.userId,
      preparedData.passAccountAddress
    );
    return result;
  }

  /**
   * This method is only available in browser environments as it uses WebAuthn APIs.
   * It performs the complete registration process by:
   * 1. Preparing registration data on the client side using WebAuthn
   * 2. Completing the registration on the server side
   * 
   * @throws {VError} If credential creation fails
   * @param user - The user object containing profile and metadata
   * @returns Promise with the registration result
   */

  async prepareRegistration(user: User<BaseProfile>) {
    const passkeysAuthenticator = await new PasskeysAuthenticator(
      user.profile.id,
      this.blockHashChallenge.bind(this),
      this.credentialsHandler
    ).setup();

    // Store the PasskeysAuthenticator for reuse in connect
    this._passkeysAuthenticator = passkeysAuthenticator;

    const passSigner = new KreivoPassSigner(passkeysAuthenticator);
    const passAccountAddress = ss58Encode(passSigner.publicKey);

    /// Registers Charlotte (esto viene en VOS)
    const client = await this.getClient();
    const finalizedBlock = await client.getFinalizedBlock();
    const attestation = await passkeysAuthenticator.register(finalizedBlock.number);

    console.log("attestation", attestation);

    const attestationJSON: AttestationData = {
      authenticator_data: attestation.authenticator_data.asHex(),
      client_data: attestation.client_data.asText(),
      public_key: attestation.public_key.asHex(),
      meta: {
        deviceId: attestation.meta.device_id.asHex(),
        context: attestation.meta.context,
        authority_id: attestation.meta.authority_id.asHex(),
      },
    };

    const credentialId = (this.credentialsHandler as VOSCredentialsHandler).getCredentialIdForUser(user.profile.id);

    if (!credentialId) {
      throw new Error("Credential ID not found");
    }

    return {
      attestation: attestationJSON,
      hashedUserId: Binary.fromBytes(passkeysAuthenticator.hashedUserId).asHex(),
      credentialId: credentialId,
      userId: user.profile.id,
      passAccountAddress
    };
  }

  async completeRegistration(
    attestation: AttestationData,
    hashedUserId: string,
    credentialId: string,
    userId: string,
    address: string
  ) {
    console.log("hashedUserId", hashedUserId);
    const postRes = await fetch(`${this.baseUrl}/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        userId: userId,
        hashedUserId,
        credentialId: credentialId,
        address,
        attestationResponse: attestation
      }),
    });

    const data = await postRes.json();
    console.log("Post-register response:", data);

    if (!postRes.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${postRes.status} ${postRes.statusText}`;
      throw new Error(`Registration failed: ${errorMessage}`);
    }

    return data;
  }

  /**
   * Resets the Auth state, clearing any stored PasskeysAuthenticator and session signer
   * This is useful when switching between different users or starting fresh
   */
  reset() {
    this._passkeysAuthenticator = null;
    this._sessionSigner = null;
  }

  /**
   * Checks if a PasskeysAuthenticator is available for use
   * 
   * @returns true if a PasskeysAuthenticator is available, false otherwise
   */
  isAuthenticated(): boolean {
    return this._passkeysAuthenticator !== null;
  }

  /**
   * Checks if a session signer is available for use
   * 
   * @returns true if a session signer is available, false otherwise
   */
  hasSessionSigner(): boolean {
    return this._sessionSigner !== null;
  }

  /**
   * Gets the stored PasskeysAuthenticator instance
   * 
   * @returns The PasskeysAuthenticator instance or null if not available
   */
  get passkeysAuthenticator(): PasskeysAuthenticator | null {
    return this._passkeysAuthenticator;
  }

  /**
   * Gets the stored session signer
   * 
   * @returns The session signer or null if not available
   */
  get sessionSigner(): any | null {
    return this._sessionSigner;
  }

  get currentUserId(): string | null {
    return this._currentUserId;
  }

  /**
   * Get the public key from a PasskeysAuthenticator as an SS58 address
   * 
   * @param sessionSigner - The session signer instance
   * @returns The SS58 encoded address
   * 
   * @example
   * const address = auth.getAddressFromAuthenticator(auth.passkeysAuthenticator);
   * console.log("User address:", address);
   */
  getAddressFromAuthenticator(sessionSigner: any): string {
    return ss58Encode(sessionSigner.publicKey);
  }

  async connect(userId: string) {
    await this.prepareConnection(userId);
    const result = await this.completeConnection();
    return result;
  }

  async prepareConnection(userId: string) {
    console.log("Connecting to user:", userId);

    if (userId) {
      this._currentUserId = userId;
    }

    console.log("Connecting to current user:", this._currentUserId);
    if (!this._currentUserId) {
      throw new Error("User ID is required");
    }
    const passkeysAuthenticator = await new PasskeysAuthenticator(
      this._currentUserId,
      this.blockHashChallenge.bind(this),
      this.credentialsHandler
    ).setup();

    this._passkeysAuthenticator = passkeysAuthenticator;

    const hashedUserId = passkeysAuthenticator.hashedUserId;

    return {
      hashedUserId: hashedUserId,
      userId: this._currentUserId,
    }
  }

  async completeConnection() {
    if (!this._passkeysAuthenticator) {
      throw new Error("PasskeysAuthenticator is not available");
    }
    const passSigner = new KreivoPassSigner(this._passkeysAuthenticator);
    const passSignerAddress = ss58Encode(passSigner.publicKey);

    // Sync nonce from blockchain before starting the connection
    if (this._nonceManager) {
      try {
        const currentNonce = await this._nonceManager.syncNonceFromChain(passSignerAddress);
        console.log(`Synced nonce for address ${passSignerAddress} before connect: ${currentNonce}`);
      } catch (error) {
        console.warn('Failed to sync nonce before connect, continuing anyway:', error);
      }
    }

    const kreivoApi = (await this.getClient()).getTypedApi(kreivo);
    // Adds a session
    const [sessionSigner, sessionKey] = passSigner.makeSessionKeySigner();
    const MINUTES = 10; // 10 blocks in a minute

    const userStartsASession = kreivoApi.tx.Pass.add_session_key({
      session: MultiAddress.Id(sessionKey),
      duration: 15 * MINUTES,
    });

    const tx3Res = await userStartsASession.signAndSubmit(passSigner, {
      mortality: { mortal: true, period: 60 }
    });
    console.log(tx3Res);

    this._sessionSigner = sessionSigner;

    const connectSignTx = await userStartsASession.sign(passSigner, {
      mortality: { mortal: true, period: 60 }
    });
    console.log("connectSignTx", connectSignTx);

    const userSessionRes = await new Promise((resolve, reject) => {
      userStartsASession.signSubmitAndWatch(passSigner, {
        mortality: { mortal: true, period: 60 }
      })
        .subscribe({
          next: async (event: any) => {
            console.info('Session transaction event:', event.type);
            if (event.type === 'txBestBlocksState') {
              // Increment nonce in NonceManager after transaction is included
              if (this._nonceManager) {
                try {
                  const passSignerAddress = ss58Encode(passSigner.publicKey);
                  console.log("nonce before", await this._nonceManager.getCurrentNonce(passSignerAddress));
                  this._nonceManager.incrementNonce(passSignerAddress);
                  console.log("nonce after", await this._nonceManager.getCurrentNonce(passSignerAddress));
                  console.log(`Incremented nonce for address ${passSignerAddress} after connect transaction inclusion`);
                } catch (error) {
                  console.warn('Failed to increment nonce after connect transaction:', error);
                }
              }
              resolve({ ok: true, txHash: event.txHash, blockHash: event.found });
            }
          },
          error: (error: unknown) => {
            console.error('Session transaction error:', error);
            reject(error);
          }
        });
    });
    console.log(userSessionRes);

    return {
      sessionKey,
      sessionSigner,
      address: passSignerAddress,
      transaction: userSessionRes
    };
  }
  /**
   * Signs a command using the user's wallet
   * This method retrieves the user's wallet from the session manager and uses it to sign the provided command
   * 
   * @param userId - The ID of the user whose wallet will be used to sign
   * @param command - The command object containing the data to be signed
   * @returns Promise with the signed extrinsic and original command data
   * @throws Will throw an error if the wallet cannot be retrieved from the session manager
   */
  async isRegistered(userId: string) {
    const res = await fetch(`${this.baseUrl}/check-user-registered?userId=${userId}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const data = await res.json();
    console.log("Is registered response:", data);

    if (!res.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${res.status} ${res.statusText}`;
      throw new Error(`Failed to check registration status: ${errorMessage}`);
    }

    return data.ok;
  }

  private blockHashChallenge = async (ctx: number, xtc: Uint8Array) => {
    const client = await this.getClient();
    const hash = await client._request("chain_getBlockHash", [ctx]);
    const blockHash = Binary.fromHex(hash);

    console.log("blockHash", blockHash.asHex());
    console.log(`BlockHashChallenger::generate(${ctx}, ${[...JSON.stringify(xtc)]})`);
    console.log("\t-> ", `${JSON.stringify([...Blake2256(mergeUint8([blockHash.asBytes(), xtc]))])}`);
    return Blake2256(mergeUint8([blockHash.asBytes(), xtc]));
  }

  public async addMember(userId: string) {
    const res = await fetch(`${this.baseUrl}/add-member`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ userId }),
    });

    const data = await res.json();
    console.log("Add member response:", data);

    if (!res.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${res.status} ${res.statusText}`;
      throw new Error(`Failed to add member: ${errorMessage}`);
    }

    return data;
  }

  public async isMember(address: string) {
    const res = await fetch(`${this.baseUrl}/is-member?address=${address}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const data = await res.json();
    console.log("Is member response:", data);

    if (!res.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${res.status} ${res.statusText}`;
      throw new Error(`Failed to check member status: ${errorMessage}`);
    }

    return data.ok;
  }

  async sign(extrinsic: string): Promise<TransactionResult> {
    if (!this._currentUserId) {
      throw new Error("User ID is not set");
    }

    try {

      const passkeysAuthenticator = await new PasskeysAuthenticator(
        this._currentUserId,
        this.blockHashChallenge.bind(this),
        this.credentialsHandler
      ).setup();
      
      this._passkeysAuthenticator = passkeysAuthenticator;

      const passSigner = new KreivoPassSigner(this._passkeysAuthenticator);
      
      const kreivoApi = (await this.getClient()).getTypedApi(kreivo);

      const transaction = await kreivoApi.txFromCallData(Binary.fromHex(extrinsic));
      const result = await transaction.signAndSubmit(passSigner);

      return {
        ok: result.ok,
        hash: result.txHash,
      };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }
}
