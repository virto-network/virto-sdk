import {
    CredentialsHandler,
} from "@virtonetwork/authenticators-webauthn";
import { arrayBufferToBase64Url } from "./utils/base64";
import { fromBase64Url } from "./utils/base64.browser";

export class VOSCredentialsHandler implements CredentialsHandler {
    private static readonly STORAGE_KEY = "vos_credentials";
    
    private static userCredentials: Record<string, Record<string, any>> = {};

    constructor(private baseURL: string) {
        console.log(this.baseURL);
        this.loadCredentialsFromStorage();
    }

    private loadCredentialsFromStorage(): void {
        try {
            const stored = localStorage.getItem(VOSCredentialsHandler.STORAGE_KEY);
            if (stored) {
                const parsed = JSON.parse(stored);
                for (const [userId, credentialId] of Object.entries(parsed)) {
                    if (typeof credentialId === 'string') {
                        const rawId = fromBase64Url(credentialId);
                        VOSCredentialsHandler.userCredentials[userId] = {
                            [credentialId]: {
                                id: credentialId,
                                rawId: rawId,
                                type: "public-key"
                            }
                        };
                    }
                }
                console.log("Loaded credentials from localStorage");
            }
            console.log("userCredentials", VOSCredentialsHandler.userCredentials);
        } catch (error) {
            console.error("Failed to load credentials:", error);
        }
    }

    private static saveCredentialsToStorage(): void {
        try {
            const simple: Record<string, string> = {};
            for (const [userId, credentials] of Object.entries(this.userCredentials)) {
                const credentialEntries = Object.keys(credentials);
                if (credentialEntries.length > 0 && credentialEntries[0]) {
                    simple[userId] = credentialEntries[0]; // Take first credential
                }
            }
            localStorage.setItem(this.STORAGE_KEY, JSON.stringify(simple));
        } catch (error) {
            console.error("Failed to save credentials:", error);
        }
    }

    private static tryMutate(userId: string, f: (credentials: Record<string, any>) => void): void {
        try {
            let map = this.userCredentials[userId] ?? {};
            f(map);
            this.userCredentials[userId] = map;
        } catch {
            /* on error, no-op */
        }
    }

    static credentialIds(userId: string): ArrayBufferLike[] {
        const credentials = this.userCredentials[userId] ?? {};
        return Object.entries(credentials).map(([, credential]) => credential.rawId);
    }

    async onCreatedCredentials(userId: string, credential: PublicKeyCredential): Promise<void> {
        VOSCredentialsHandler.tryMutate(userId, (credentials) => {
            credentials[credential.id] = credential;
        });
        
        VOSCredentialsHandler.saveCredentialsToStorage();
        
        console.log("Saved credential for user:", userId);
    }

    publicKeyCreateOptions = async (
        challenge: Uint8Array,
        user: PublicKeyCredentialUserEntity
    ): Promise<CredentialCreationOptions["publicKey"]> => {
        console.log("user", user);
        const queryParams = new URLSearchParams({
            id: user.name,
            challenge: arrayBufferToBase64Url(challenge),
            ...(user.name && { name: user.name })
        });
        console.log("challenge array buffer", arrayBufferToBase64Url(challenge));
        const res = await fetch(`${this.baseURL}/attestation?${queryParams}`, {
            method: "GET",
            headers: { "Content-Type": "application/json" },
        });
        const response = await res.json();
        console.log("Server response:", response);

        const publicKey = response;
        publicKey.challenge = challenge;
        publicKey.user.id = new Uint8Array(publicKey.user.id);
        
        return publicKey;
    }

    publicKeyRequestOptions = async (
        userId: string,
        challenge: Uint8Array
    ): Promise<CredentialRequestOptions["publicKey"]> => {
        console.log("userId", userId);
        console.log("userId base64", arrayBufferToBase64Url(userId));

        const queryParams = new URLSearchParams({
            userId,
            challenge: arrayBufferToBase64Url(challenge),
        });

        const res = await fetch(`${this.baseURL}/assertion?${queryParams}`, {
            method: "GET",
            headers: { "Content-Type": "application/json" },
        });
        const response = await res.json();
        console.log("Server response:", response);

        const publicKey = response;

        publicKey.allowCredentials[0].id = fromBase64Url(response.allowCredentials[0].id);
        publicKey.challenge = challenge;

        console.log("publicKey", publicKey);

        return publicKey;
    }

    getCredentialIdForUser(userId: string): string | null {
        console.log("Getting credential for user:", userId);
        const credentials = VOSCredentialsHandler.userCredentials[userId] ?? {};
        const credentialIds = Object.keys(credentials);
        
        if (credentialIds.length > 0) {
            const credentialId = credentialIds[0];
            console.log("Found credential ID:", credentialId);
            return credentialId || null;
        }
        
        console.log("No credential found for user");
        return null;
    }
}