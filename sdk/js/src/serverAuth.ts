import { PreparedRegistrationData } from "./auth";
import ServerManager from "./serverManager";
import { Command, WalletType } from "./types";
import { VError } from "./utils/error";
import jwt, { Secret, SignOptions } from 'jsonwebtoken';

export interface PreparedConnectionData {
  userId: string;
  assertionResponse: {
    id: string;
    rawId: string;
    type: string;
    response: {
      authenticatorData: string;
      clientDataJSON: string;
      signature: string;
    }
  };
  blockNumber: number;
}

export interface JWTPayload {
  userId: string;
  address: string;
  exp: number;
  iat: number;
}

/**
 * Server version of the Auth class
 * This class only contains methods that DO NOT depend on browser APIs
 * and can be executed in a Node.js environment
 */
export default class ServerAuth {
  private _jwtSecret: Secret | null = null;
  private _jwtExpiresIn: string = "10m"; // Default 10 minutes

  /**
   * Creates a new ServerAuth instance
   * 
   * @param baseUrl - The base URL of the federate server
   * @param sessionManager - The server session manager for handling user sessions
   * @param defaultWalletType - The default wallet type to use when creating new sessions
   * @param jwtConfig - JWT configuration for token generation and verification
   */
  constructor(
    private readonly baseUrl: string,
    private readonly sessionManager: ServerManager,
    private readonly defaultWalletType: WalletType = WalletType.POLKADOT,
    jwtConfig?: {
      secret: string | Secret;
      expiresIn?: string;
    }
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
   * @param address - The wallet address of the user
   * @returns The generated JWT token
   * @throws Error if JWT configuration is not available
   */
  private generateToken(userId: string, address: string): string {
    if (!this._jwtSecret) {
      throw new VError("E_NO_JWT_CONFIG", "JWT configuration not provided");
    }

    return jwt.sign(
      { userId, address },
      this._jwtSecret,
      { expiresIn: this._jwtExpiresIn } as SignOptions
    );
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
   * Signs a command after verifying the JWT token
   * 
   * @param token - The JWT token to verify
   * @param command - The command to sign
   * @returns The signed command
   * @throws VError if the token is invalid or expired
   */
  async signWithToken(token: string, command: Command) {
    const payload = this.verifyToken(token);

    const session = this.sessionManager.getSession(payload.userId);
    if (!session) {
      throw new VError("E_SESSION_NOT_FOUND", "Session not found for this token");
    }

    if (session.address !== payload.address) {
      throw new VError("E_ADDRESS_MISMATCH", "Token address does not match session address");
    }

    return this.sign(payload.userId, command);
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
  async completeConnection(preparedData: PreparedConnectionData) {
    const sessionPreparationRes = await fetch(`${this.baseUrl}/connect`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(preparedData),
    });

    const data = await sessionPreparationRes.json();
    console.log("Post-connect response:", data);

    const sessionResult = await this.sessionManager.create(
      data.command,
      preparedData.userId,
      this.defaultWalletType
    );

    let token = null;

    // Generate JWT token if JWT configuration is available
    if (this._jwtSecret && sessionResult.session) {
      try {
        token = this.generateToken(
          preparedData.userId,
          sessionResult.session.address
        );
      } catch (error) {
        console.warn("Token generation failed, continuing without JWT:", error);
      }
    }

    return {
      ...data,
      ...sessionResult,
      token
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

    return data.ok;
  }

  /**
   * Signs a command using the user's wallet
   * This method retrieves the user's wallet from the session manager and uses it to sign the provided command
   * 
   * The method:
   * 1. Retrieves the user's wallet from the session manager
   * 2. Uses the wallet to sign the provided command
   * 3. Returns the signing result including the original and signed data
   * 
   * @param userId - The ID of the user whose wallet will be used to sign
   * @param command - The command object containing the data to be signed
   * @returns Promise with the signing result including user ID, signed extrinsic, and original command
   * @throws {VError} If the wallet cannot be retrieved from the session manager
   */
  private async sign(userId: string, command: Command) {
    const wallet = this.sessionManager.getWallet(userId);
    console.log({ wallet })

    if (!wallet) {
      throw new VError("E_CANT_GET_CREDENTIAL", "User wallet not found");
    }

    const signedExtrinsic = await wallet.sign(command);

    return {
      userId,
      signedExtrinsic,
      originalExtrinsic: command.hex
    };
  }
} 