import { VError } from "./utils/error";
import { fromBase64 } from "./utils/base64";

export type BaseProfile = {
  id: string;
  name: string;
  displayName: string;
};

export type User<Profile, Metadata extends Record<string, unknown>> = {
  profile: Profile;
  metadata: Metadata;
};

export default class Auth {
  baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  async register<Profile extends BaseProfile, Metadata extends Record<string, unknown>>(
    user: User<Profile, Metadata>
  ) {
    const preRes = await fetch(`${this.baseUrl}/pre-register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(user),
    });

    const attestation = await preRes.json();
    console.log("Pre-register response:", attestation);

    attestation.publicKey.challenge = fromBase64(attestation.publicKey.challenge);
    attestation.publicKey.user.id = fromBase64(attestation.publicKey.user.id);

    const response = await navigator.credentials.create(attestation);
    console.log("Credential response:", response);

    if (!response) {
      throw new VError("E_CANT_CREATE_CREDENTIAL", "Credential creation failed");
    }

    const postRes = await fetch(`${this.baseUrl}/post-register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: response.id }),
    });

    const data = await postRes.json();
    console.log("Post-register response:", data);

    return data;
  }
}


