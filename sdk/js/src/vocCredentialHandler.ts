import {
    CredentialsHandler,
} from "@virtonetwork/authenticators-webauthn";
import { arrayBufferToBase64Url, hexToUint8Array } from "./utils/base64";

const RP_NAME = 'Example RP';

async function hashUserId(userId: string): Promise<Uint8Array> {
    try {
        return new Uint8Array(
            await crypto.subtle.digest(
                "SHA-256",
                new TextEncoder().encode(userId)
            )
        );
    } catch (error) {
        console.error('Error hashing user ID:', error);
        throw new Error('Failed to hash user ID');
    }
}

export class VOSCredentialsHandler implements CredentialsHandler {
    constructor(private baseURL: string) {
        console.log(this.baseURL);
    }

    publicKeyCreateOptions = async (
        challenge: Uint8Array,
        user: PublicKeyCredentialUserEntity
    ): Promise<CredentialCreationOptions["publicKey"]> => {
        // const queryParams = new URLSearchParams({
        //     id: arrayBufferToBase64Url(user.id),
        //     challenge: arrayBufferToBase64Url(challenge),
        //     ...(user.name && { name: user.name })
        // });
        // console.log(arrayBufferToBase64Url(challenge));
        // const res = await fetch(`${this.baseURL}/attestation?${queryParams}`, {
        //     method: "GET",
        //     headers: { "Content-Type": "application/json" },
        // });
        // const response = await res.json();
        // console.log("Server response:", response);

        // const publicKeyOptions = response.publicKey;

        // publicKeyOptions.challenge = challenge;
        // if (Array.isArray(publicKeyOptions.user.id)) {
        //     publicKeyOptions.user.id = new Uint8Array(publicKeyOptions.user.id);
        // }
        const hashedUserId = await hashUserId(user.id.toString());
        const userIdArray = Array.from(hashedUserId);
        const publicKey = {
            rp: {
                name: RP_NAME,
            },
            user: {
                id: new Uint8Array(userIdArray),
                name: user.id.toString(),
                displayName: user.name as string ?? user.id.toString(),
            },
            challenge,
            pubKeyCredParams: [{ type: "public-key", alg: -7 }],
            authenticatorSelection: { userVerification: "preferred" },
            timeout: 60000,
            attestation: "none",
        } as unknown as PublicKeyCredentialCreationOptions;

        console.log(publicKey);

        return publicKey;
    }

    onCreatedCredentials = async (
        _userId: string,
        _credentials: PublicKeyCredential
    ): Promise<void> => { }

    publicKeyRequestOptions = async (
        userId: string,
        challenge: Uint8Array
    ): Promise<CredentialRequestOptions["publicKey"]> => {
        const queryParams = new URLSearchParams({
            userId: userId,
            challenge: arrayBufferToBase64Url(challenge),
        });
        return fetch(`${this.baseURL}/assertion?${queryParams}`, {
            body: JSON.stringify({
                challenge: arrayBufferToBase64Url(challenge),
            }),
        }).then((response) => {
            return response.json() as Promise<CredentialRequestOptions["publicKey"]>;
        });
    }
}