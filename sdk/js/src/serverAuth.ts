import { JWTPayload, AttestationData, SignFn } from "./types";
import { VError } from "./utils/error";
import jwt, { Secret, SignOptions } from 'jsonwebtoken';
import { kreivo, MultiAddress } from "@virtonetwork/sdk/descriptors";
import { Binary, PolkadotClient } from "polkadot-api";
import { Blake2256 } from '@polkadot-api/substrate-bindings';
import { mergeUint8 } from "polkadot-api/utils";

import { PolkadotSigner } from "polkadot-api";
import {
  entropyToMiniSecret,
  generateMnemonic,
  mnemonicToEntropy,
  ss58Encode,
} from "@polkadot-labs/hdkd-helpers";

import { getPolkadotSigner } from "polkadot-api/signer";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { IStorage } from "./storage";
import { SerializableSignerData, SignerSerializer } from "./storage/SignerSerializer";

/**
 * Server version of the Auth class
 * This class only contains methods that DO NOT depend on browser APIs
 * and can be executed in a Node.js environment
 */
export default class ServerAuth {
  private _jwtSecret: Secret | null = null;
  private _jwtExpiresIn: string = "10m"; // Default 10 minutes
  private _client: PolkadotClient | null = null;
  private _sessionSigner: PolkadotSigner & {
    sign: SignFn;
  } | null = null;

  /**
   * Creates a new ServerAuth instance
   * 
   * @param baseUrl - The base URL of the federate server
   * @param clientFactory - Factory function to get the Polkadot client
   * @param storage - Storage implementation for sessions
   * @param jwtConfig - JWT configuration for token generation and verification
   */
  constructor(
    private readonly baseUrl: string,
    private readonly clientFactory: () => Promise<PolkadotClient>,
    private readonly storage?: IStorage<SerializableSignerData>,
    jwtConfig?: {
      secret: string | Secret;
      expiresIn?: string;
    },
  ) {
    if (jwtConfig) {
      this._jwtSecret = jwtConfig.secret;
      if (jwtConfig.expiresIn) {
        this._jwtExpiresIn = jwtConfig.expiresIn;
      }
    }
  }

  /**
   * Generates a JWT token for a user
   * 
   * @param userId - The ID of the user
   * @param publicKey - The public key of the user
   * @returns The generated JWT token
   * @throws Error if JWT configuration is not available
   */
  private generateToken(userId: string, publicKey: string, address: string): string {
    if (!this._jwtSecret) {
      throw new VError("E_NO_JWT_CONFIG", "JWT configuration not provided");
    }

    return jwt.sign(
      { userId, publicKey, address },
      this._jwtSecret,
      { expiresIn: this._jwtExpiresIn } as SignOptions
    );
  }

  private async getClient(): Promise<PolkadotClient> {
    if (!this._client) {
      this._client = await this.clientFactory();
    }
    return this._client;
  }

  /**
   * Static method to extract the server address from a session signer
   * This is used by the NonceManager to get the correct address for nonce calculation
   * 
   * @param sessionSigner - The session signer with the stored address
   * @returns The original address of the session signer
   */
  static getServerSignerAddress(sessionSigner: any): string {
    if (sessionSigner && (sessionSigner as any)._serverAddress) {
      return (sessionSigner as any)._serverAddress;
    }
    throw new Error("Session signer does not have a stored address");
  }

  /**
   * Verifies a JWT token
   * 
   * @param token - The JWT token to verify
   * @returns The decoded token payload if valid
   * @throws VError if the token is invalid or expired
   */
  verifyToken(token: string): JWTPayload {
    if (!this._jwtSecret) {
      throw new VError("E_NO_JWT_CONFIG", "JWT configuration not provided");
    }

    try {
      return jwt.verify(token, this._jwtSecret) as JWTPayload;
    } catch (error: any) {
      if (error.name === 'TokenExpiredError') {
        throw new VError("E_JWT_EXPIRED", "Token has expired");
      } else if (error.name === 'JsonWebTokenError') {
        throw new VError("E_JWT_INVALID", "Invalid token");
      }
      throw new VError("E_JWT_UNKNOWN", "Token verification failed");
    }
  }

  /**
   * Decodes a JWT token without verifying its signature
   * This method extracts the payload information without validation
   * 
   * @param token - The JWT token to decode
   * @returns The decoded token payload
   * @throws VError if the token format is invalid
   */
  public decodeToken(token: string): JWTPayload {
    try {
      const decoded = jwt.decode(token) as JWTPayload;
      
      if (!decoded) {
        throw new VError("E_JWT_INVALID_FORMAT", "Token format is invalid");
      }

      return decoded;
    } catch (error: any) {
      if (error instanceof VError) {
        throw error;
      }
      throw new VError("E_JWT_DECODE_FAILED", "Failed to decode token");
    }
  }

  /**
   * Signs a command after verifying the JWT token
   * Signs directly without using the transaction queue
   * 
   * @param token - The JWT token to verify
   * @param extrinsic - The extrinsic to sign
   * @returns The signed command result
   * @throws VError if the token is invalid or expired
   */
  async signWithToken(token: string, extrinsic: string) {
    const payload = this.verifyToken(token);

    if (this.storage && !this._sessionSigner) {
      const storedData = await this.storage.get(payload.userId);
      if (storedData && SignerSerializer.isSerializableSignerData(storedData)) {
        this._sessionSigner = SignerSerializer.deserialize(storedData);
      }
    }

    if (!this._sessionSigner) {
      throw new VError("E_SESSION_NOT_FOUND", "Session not found");
    }

    if (Binary.fromBytes(this._sessionSigner.publicKey).asHex() !== payload.publicKey) {
      throw new VError("E_ADDRESS_MISMATCH", "Token address does not match session address");
    }

    const kreivoApi = (await this.getClient()).getTypedApi(kreivo);
    const transaction = await kreivoApi.txFromCallData(Binary.fromHex(extrinsic));
    const result = await transaction.signAndSubmit(this._sessionSigner);

    return {
      ok: result.ok,
      hash: result.txHash,
      blockHash: result.block?.hash
    };
  }

  /**
   * Completes the registration process on the server side
   * This method is designed to run in a Node.js environment
   * 
   * The method:
   * 1. Sends the prepared registration data to the federate server
   * 2. Processes the response
   * 
   * @param preparedData - The registration data prepared by the client, including:
   *   - userId: The unique identifier for the user
   *   - attestationResponse: The WebAuthn credential data
   *   - blockNumber: The blockchain block number for registration
   * @returns Promise with the server's response to the registration
   * @throws Will throw an error if the server request fails
   */
  async completeRegistration(
    attestation: AttestationData,
    hashedUserId: string,
    credentialId: string,
    userId: string,
    address: string
  ) {
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
   * Completes the connection process on the server side
   * This method is designed to run in a Node.js environment
   * 
   * The method:
   * 1. Sends the prepared connection data to the federate server
   * 2. Creates a session for the user using the session manager
   * 3. Returns both the server response and session information
   * 4. Generates a JWT token if JWT configuration is available
   * 
   * @param preparedData - The connection data prepared by the client, including:
   *   - userId: The unique identifier for the user
   *   - assertionResponse: The WebAuthn assertion response
   *   - blockNumber: The blockchain block number for connection
   * @returns Promise with the server's response, session information, and auth token
   * @throws Will throw an error if the server request fails or session creation fails
   */
  async completeConnection(userId: string) {
    if (!userId) {
      throw new Error("User ID is required");
    }

    const kreivoApi = (await this.getClient()).getTypedApi(kreivo);

    const miniSecret = entropyToMiniSecret(
      mnemonicToEntropy(generateMnemonic(256))
    );
    const derive = sr25519CreateDerive(miniSecret);

    const derivationPath = `//${Date.now()}`;
    const keypair = derive(derivationPath);

    const signer = getPolkadotSigner(keypair.publicKey, "Sr25519", keypair.sign);
    Object.defineProperty(signer, "sign", {
      value: keypair.sign,
      configurable: false,
    });

    // Adds a session
    const { s, address } = {
      s: signer as PolkadotSigner & { sign: SignFn },
      address: ss58Encode(keypair.publicKey, 2),
    };

    const hashedUserId = new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(userId))
    );

    s.publicKey = Blake2256(
      mergeUint8(new Uint8Array(32).fill(0), hashedUserId)
    );

    console.log("s.publicKey", s.publicKey);
    console.log("address", address);

    const MINUTES = 10; // 10 blocks in a minute

    console.log("Adding session key");
    const userStartsASession = kreivoApi.tx.Pass.add_session_key({
      session: MultiAddress.Id(address),
      duration: 15 * MINUTES,
    });

    this._sessionSigner = s;

    if (this.storage) {
      // Serialize the signer for storage
      const serializableData = SignerSerializer.serialize({
        miniSecret,
        derivationPath,
        originalPublicKey: keypair.publicKey,
        hashedUserId,
        address
      });

      await this.storage.store(userId, serializableData);
    }

    let token: string | null = null;
    // Generate JWT token if JWT configuration is available
    if (this._jwtSecret) {
      try {
        token = this.generateToken(
          userId,
          Binary.fromBytes(s.publicKey).asHex(),
          address
        );
      } catch (error) {
        console.warn("Token generation failed, continuing without JWT:", error);
      }
    }

    const responseStartsASession = await userStartsASession.getEncodedData();

    return {
      ok: true,
      extrinsic: responseStartsASession.asHex(),
      token,
      publicKey: Binary.fromBytes(s.publicKey).asHex(),
    };
  }

  /**
   * Checks if a user is registered with the federate server
   * This method is a utility to verify registration status
   * 
   * @param userId - The unique identifier of the user to check
   * @returns Promise with a boolean indicating if the user is registered
   * @throws Will throw an error if the server request fails
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
}
