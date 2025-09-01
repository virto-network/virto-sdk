import { BaseProfile, User } from "./types";
import { Blake2256 } from "@polkadot-api/substrate-bindings";
import { mergeUint8 } from "polkadot-api/utils";
import { kreivo, MultiAddress } from "@polkadot-api/descriptors";
import { Binary, PolkadotClient } from "polkadot-api";
import {
  CredentialsHandler,
  WebAuthn as PasskeysAuthenticator,
} from "@virtonetwork/authenticators-webauthn";
import { KreivoPassSigner } from "@virtonetwork/signer";
import { ss58Encode } from "@polkadot-labs/hdkd-helpers";
import { VOSCredentialsHandler } from "./vocCredentialHandler";

let base64Module: typeof import("./utils/base64.browser") | null = null;

async function loadBase64Module() {
  if (!base64Module && typeof window !== "undefined") {
    base64Module = await import("./utils/base64.browser");
  }
  return base64Module;
}

export interface PreparedCredentialData {
  id: string;
  rawId: string;
  type: string;
  response: {
    authenticatorData: string;
    clientDataJSON: string;
    publicKey: string;
  }
}

export interface PreparedRegistrationData {
  userId: string;
  attestationResponse: PreparedCredentialData;
  blockNumber: number;
}

export default class Auth {
  private _client: PolkadotClient | null = null;
  private _passkeysAuthenticator: PasskeysAuthenticator | null = null;
  private _sessionSigner: any | null = null;

  constructor(
    private readonly baseUrl: string,
    private readonly credentialsHandler: CredentialsHandler,
    private readonly clientFactory: () => Promise<PolkadotClient>,
  ) {
    // Preload the module if we're in a browser environment
    if (typeof window !== "undefined") {
      loadBase64Module();
    }
  }

  private async getClient(): Promise<PolkadotClient> {
    if (!this._client) {
      this._client = await this.clientFactory();
    }
    return this._client;
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
  
  async register<Profile extends BaseProfile>(
    user: User<Profile>
  ) {
    const passkeysAuthenticator = await new PasskeysAuthenticator(
      user.profile.id,
      this.blockHashChallenge.bind(this),
      this.credentialsHandler
    ).setup();
    
    // Store the PasskeysAuthenticator for reuse in connect
    this._passkeysAuthenticator = passkeysAuthenticator;
    
    const passSigner = new KreivoPassSigner(passkeysAuthenticator);
    const passAccountAddress = ss58Encode(passSigner.publicKey);

    console.log("before get client")

    /// Registers Charlotte (esto viene en VOS)
    const client = await this.getClient();
    const finalizedBlock = await client.getFinalizedBlock();
    const attestation = await passkeysAuthenticator.register(finalizedBlock.number);

    console.log("attestation", attestation);

    const attestationJSON = {
      authenticator_data: attestation.authenticator_data.asHex(),
      client_data: attestation.client_data.asText(),
      public_key: attestation.public_key.asHex(),
      meta: {
        deviceId: attestation.meta.device_id.asHex(),
        context: attestation.meta.context,
        authority_id: attestation.meta.authority_id.asHex(),
      },
    };

    console.log("hashedUserId", passkeysAuthenticator.hashedUserId);
    console.log("credentialId", (this.credentialsHandler as VOSCredentialsHandler).getCredentialIdForUser(user.profile.id));
    const postRes = await fetch(`${this.baseUrl}/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ 
        userId: user.profile.id, 
        hashedUserId: Binary.fromBytes(passkeysAuthenticator.hashedUserId).asHex(), 
        credentialId: (this.credentialsHandler as VOSCredentialsHandler).getCredentialIdForUser(user.profile.id),
        address: passAccountAddress,
        attestationResponse: attestationJSON
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

  /**
   * This method is only available in browser environments as it uses WebAuthn APIs.
   * It performs the complete connection process by:
   * 1. Preparing connection data on the client side using WebAuthn
   * 2. Completing the connection on the server side
   * 
   * @throws {VError} If credential retrieval fails
   * @param userId - The user ID to connect
   /**
    * Connects a user using WebAuthn and sets up a session key.
    * @param userId - The user ID to connect
    * @returns Promise<{ sessionKey: string, sessionSigner: any, transaction: any }>
    */
   async connect(userId: string): Promise<{ sessionKey: string, sessionSigner: any, transaction: any }> {
    console.log("Connecting to user:", userId);
    
    const passkeysAuthenticator = await new PasskeysAuthenticator(
      userId,
      this.blockHashChallenge.bind(this),
      this.credentialsHandler
    ).setup();

    this._passkeysAuthenticator = passkeysAuthenticator;
    
    const passSigner = new KreivoPassSigner(passkeysAuthenticator);

    const kreivoApi = (await this.getClient()).getTypedApi(kreivo);
    // Adds a session
    const [sessionSigner, sessionKey] = passSigner.makeSessionKeySigner();
    const MINUTES = 10; // 10 blocks in a minute

    const charlotteStartsASession = kreivoApi.tx.Pass.add_session_key({
      session: MultiAddress.Id(sessionKey),
      duration: 15 * MINUTES,
    });

    const tx3Res = await charlotteStartsASession.signAndSubmit(passSigner, { 
      mortality: { mortal: true, period: 60 }
    });
    console.log(tx3Res);
    
    // Store the session signer for later use
    this._sessionSigner = sessionSigner;
    
    return {
      sessionKey,
      sessionSigner,
      transaction: tx3Res
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

    console.log(`BlockHashChallenger::generate(${ctx}, ${[...JSON.stringify(xtc)]})`);
    console.log("\t-> ", `${JSON.stringify([...Blake2256(mergeUint8([blockHash.asBytes(), xtc]))])}`);
    return Blake2256(mergeUint8([blockHash.asBytes(), xtc]));
  }
}
