import { VError } from "./utils/error";
import { arrayBufferToBase64Url, hexToUint8Array } from "./utils/base64";
import SessionManager from "./manager";
import { WalletType } from "./factory/walletFactory";
import { BaseProfile, Command, User } from "./types";

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
  constructor(
    private readonly baseUrl: string,
    private readonly sessionManager: SessionManager,
    private readonly defaultWalletType: WalletType
  ) {
    // Preload the module if we're in a browser environment
    if (typeof window !== "undefined") {
      loadBase64Module();
    }
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
    const preparedData = await this.prepareRegistration(user);
    return this.completeRegistration(preparedData);
  }

  /**
   * Prepares registration data on the client side using WebAuthn APIs.
   * This method can only be called from a browser environment as it uses
   * the WebAuthn API (navigator.credentials).
   * 
   * The method:
   * 1. Fetches attestation options from the server
   * 2. Creates a new credential using WebAuthn
   * 3. Formats the credential data for server submission
   * 
   * @throws {VError} If credential creation fails
   * @param user - The user object containing profile and metadata
   * @returns Promise with the prepared registration data
   */
  async prepareRegistration<Profile extends BaseProfile>(
    user: User<Profile>
  ): Promise<PreparedRegistrationData> {
    const queryParams = new URLSearchParams({
      id: user.profile.id,
      ...(user.profile.name && { name: user.profile.name })
    });
    const preRes = await fetch(`${this.baseUrl}/attestation?${queryParams}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    const attestation = await preRes.json();
    console.log("Pre-register response:", attestation);

    attestation.publicKey.challenge = hexToUint8Array(attestation.publicKey.challenge);
    attestation.publicKey.user.id = new Uint8Array(attestation.publicKey.user.id);

    const attestationResponse = await navigator.credentials.create(attestation);

    if (!attestationResponse) {
      throw new VError("E_CANT_CREATE_CREDENTIAL", "Credential creation failed");
    }

    const { id } = attestationResponse;
    const rawId = (attestationResponse as PublicKeyCredential).rawId;
    const response = (attestationResponse as PublicKeyCredential).response as AuthenticatorAttestationResponse;
    const authenticatorData = response.getAuthenticatorData();
    const clientDataJSON = response.clientDataJSON;
    const publicKey = response.getPublicKey();

    const credentialData: PreparedCredentialData = {
      id,
      rawId: arrayBufferToBase64Url(rawId),
      type: attestationResponse.type,
      response: {
        authenticatorData: arrayBufferToBase64Url(authenticatorData),
        clientDataJSON: arrayBufferToBase64Url(clientDataJSON),
        publicKey: arrayBufferToBase64Url(publicKey),
      }
    };

    return {
      userId: user.profile.id,
      attestationResponse: credentialData,
      blockNumber: attestation.blockNumber
    };
  }

  /**
   * Completes the registration process on the server side
   * This method is designed to run in a Node.js environment as it doesn't use any browser APIs
   * 
   * @param preparedData - The registration data prepared by the client, including:
   *   - userId: The unique identifier for the user
   *   - attestationResponse: The WebAuthn credential data
   *   - blockNumber: The blockchain block number for registration
   * @returns Promise with the server's response to the registration
   * @throws Will throw an error if the server request fails
   */
  async completeRegistration(preparedData: PreparedRegistrationData) {
    const postRes = await fetch(`${this.baseUrl}/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(preparedData),
    });

    const data = await postRes.json();
    console.log("Post-register response:", data);

    return data;
  }

  /**
   * This method is only available in browser environments as it uses WebAuthn APIs.
   * It performs the complete connection process by:
   * 1. Preparing connection data on the client side using WebAuthn
   * 2. Completing the connection on the server side
   * 
   * @throws {VError} If credential retrieval fails
   * @param userId - The user ID to connect
   * @returns Promise with the connection result
   */
  async connect(userId: string) {
    const preparedData = await this.prepareConnection(userId);
    return this.completeConnection(preparedData);
  }

  /**
   * Prepares connection data on the client side using WebAuthn APIs.
   * This method can only be called from a browser environment as it uses
   * the WebAuthn API (navigator.credentials).
   * 
   * The method:
   * 1. Fetches assertion options from the server
   * 2. Gets existing credential using WebAuthn
   * 3. Formats the credential data for server submission
   * 
   * @throws {VError} If credential retrieval fails
   * @param userId - The user ID to connect
   * @returns Promise with the prepared connection data
   */
  async prepareConnection(userId: string) {
    const base64 = await loadBase64Module();

    if (!base64) {
      throw new VError("E_ENVIRONMENT", "This method can only be called in a browser environment");
    }

    const preRes = await fetch(`${this.baseUrl}/assertion?userId=${userId}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const assertion = await preRes.json();
    console.log("Connect response:", assertion);

    assertion.publicKey.challenge = hexToUint8Array(assertion.publicKey.challenge);

    if (assertion.publicKey.allowCredentials) {
      for (const desc of assertion.publicKey.allowCredentials) {
        desc.id = base64.fromBase64Url(desc.id);
      }
    }

    const assertionResponse = await navigator.credentials.get(assertion);
    console.log("Credential response:", assertionResponse);

    if (!assertionResponse) {
      throw new VError("E_CANT_GET_CREDENTIAL", "Credential retrieval failed");
    }
    const { id, rawId, response } = assertionResponse as PublicKeyCredential;
    const { authenticatorData, clientDataJSON, signature } = response as AuthenticatorAssertionResponse;
    const credentialData = {
      id,
      rawId: arrayBufferToBase64Url(rawId),
      type: assertionResponse.type,
      response: {
        authenticatorData: arrayBufferToBase64Url(authenticatorData),
        clientDataJSON: arrayBufferToBase64Url(clientDataJSON),
        signature: arrayBufferToBase64Url(signature),
      }
    }

    return {
      userId,
      assertionResponse: credentialData,
      blockNumber: assertion.blockNumber
    };
  }

  /**
   * Completes the connection process on the server side
   * This method sends the prepared connection data to the server and establishes a session
   * 
   * @param preparedData - The connection data prepared by the client
   * @returns Promise with the server's response and session information
   * @throws Will throw an error if the server request fails
   */
  async completeConnection(preparedData: {
    userId: string;
    assertionResponse: any;
    blockNumber: number;
  }) {
    const sessionPreparationRes = await fetch(`${this.baseUrl}/connect`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(preparedData),
    });

    const data = await sessionPreparationRes.json();
    console.log("Post-connect response:", data);

    const sessionResult = await this.sessionManager.create(data.command, preparedData.userId, this.defaultWalletType);

    return {
      ...data,
      ...sessionResult
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

    return data.ok;
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
  async sign(userId: string, command: Command) {
    const wallet = await this.sessionManager.getWallet(userId);
    console.log({ wallet })
    if (!wallet) {
      throw new VError("E_CANT_GET_CREDENTIAL", "Credential retrieval failed");
    }

    const signedExtrinsic = await wallet.sign(command);

    return {
      userId,
      signedExtrinsic,
      originalExtrinsic: command.hex
    };
  }
}
