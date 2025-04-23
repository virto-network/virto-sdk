import { VError } from "./utils/error";
import { arrayBufferToBase64Url, fromBase64Url, hexToUint8Array } from "./utils/base64";
import SessionManager from "./manager";
import { WalletType } from "./factory/walletFactory";
import { BaseProfile, Command, User } from "./types";

export default class Auth {
  constructor(
    private readonly baseUrl: string,
    private readonly sessionManager: SessionManager,
    private readonly defaultWalletType: WalletType
  ) {
  }

  async register<Profile extends BaseProfile>(
    user: User<Profile>
  ) {
    const queryParams = new URLSearchParams({
      id: user.profile.id,
      ...(user.profile.name && { name: user.profile.name })
    });
    const preRes = await fetch(`${this.baseUrl}/api/attestation?${queryParams}`, {
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

    const credentialData = {
      id,
      rawId: arrayBufferToBase64Url(rawId),
      type: attestationResponse.type,
      response: {
        authenticatorData: arrayBufferToBase64Url(authenticatorData),
        clientDataJSON: arrayBufferToBase64Url(clientDataJSON),
        publicKey: arrayBufferToBase64Url(publicKey),
      }
    };

    const postRes = await fetch(`${this.baseUrl}/api/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        userId: user.profile.id,
        attestationResponse: credentialData,
        blockNumber: attestation.blockNumber
      }),
    });

    const data = await postRes.json();
    console.log("Post-register response:", data);

    return data;
  }

  async connect(userId: string) {
    const preRes = await fetch(`${this.baseUrl}/api/assertion?userId=${encodeURIComponent(userId)}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const assertion = await preRes.json();
    console.log("Connect response:", assertion);

    assertion.publicKey.challenge = hexToUint8Array(assertion.publicKey.challenge);

    if (assertion.publicKey.allowCredentials) {
      for (const desc of assertion.publicKey.allowCredentials) {
        desc.id = fromBase64Url(desc.id);
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

    const sessionPreparationRes = await fetch(`${this.baseUrl}/api/connect`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        userId,
        assertionResponse: credentialData,
        blockNumber: assertion.blockNumber
      }),
    });

    const data = await sessionPreparationRes.json();
    console.log("Post-connect response:", data);

    const sessionResult = await this.sessionManager.create(data.command, userId, this.defaultWalletType);

    return {
      ...data,
      ...sessionResult
    };
  }

  async isRegistered(userId: string) {
    const res = await fetch(`${this.baseUrl}/check-user-registered?username=${encodeURIComponent(userId)}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const data = await res.json();
    console.log("Is registered response:", data);

    return data.ok;
  }

  async sign(userId: string, command: Command) {
    const wallet = this.sessionManager.getWallet(userId);
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
