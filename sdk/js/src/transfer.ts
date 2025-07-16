import { kreivo, MultiAddress } from "@polkadot-api/descriptors";
import { PolkadotClient } from "polkadot-api";
import { WebAuthn as PasskeysAuthenticator } from "@virtonetwork/authenticators-webauthn";
import { KreivoPassSigner } from "@virtonetwork/signer";
import { ss58Encode, ss58Decode } from "@polkadot-labs/hdkd-helpers";

export interface TransferOptions {
  dest: string; // SS58 address
  value: bigint; // Amount in units
}

export interface TransferByUsernameOptions {
  username: string; // Username to resolve to address
  value: bigint; // Amount in units
}

export interface SendAllOptions {
  dest: string; // SS58 address
  keepAlive?: boolean; // Whether to keep account alive (default: true)
}

export interface SendAllByUsernameOptions {
  username: string; // Username to resolve to address
  keepAlive?: boolean; // Whether to keep account alive (default: true)
}

export interface BalanceInfo {
  free: bigint;
  reserved: bigint;
  frozen: bigint;
  total: bigint;
  transferable: bigint;
}

export interface TransferResult {
  hash: string;
  blockHash?: string;
  success: boolean;
  error?: string;
}

export interface UserInfo {
  address: string;
  username: string;
}

// Type for unsigned extrinsics from transfer module
export type TransferExtrinsic = any;

export interface UserService {
  getUserAddress(username: string): Promise<string>;
}

export class DefaultUserService implements UserService {
  constructor(private baseURL: string) {}

  async getUserAddress(username: string): Promise<string> {
    try {
      const queryParams = new URLSearchParams({
        userId: username,
      });

      const res = await fetch(`${this.baseURL}/get-user-address?${queryParams}`, {
        method: "GET",
        headers: { "Content-Type": "application/json" },
      });

      if (!res.ok) {
        throw new Error(`HTTP error! status: ${res.status}`);
      }

      const response = await res.json();
      
      if (!response.address) {
        throw new Error("User address not found in response");
      }

      return response.address;
    } catch (error) {
      console.error("Failed to get user address:", error);
      throw new Error(
        `Failed to resolve username "${username}" to address: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
  }
}

export default class Transfer {
  private userService: UserService | null = null;

  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>,
    userService?: UserService
  ) {
    this.userService = userService || null;
  }

  setUserService(userService: UserService): void {
    this.userService = userService;
  }

  private async getClient(): Promise<PolkadotClient> {
    return await this.clientFactory();
  }

  private async resolveUsernameToAddress(username: string): Promise<string> {
    if (!this.userService) {
      throw new Error("User service not configured. Please set a user service to resolve usernames.");
    }
    return await this.userService.getUserAddress(username);
  }

  /**
   * Transfer a specific amount to a destination address
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Transfer options including destination and amount
   * @returns Promise with the transaction result
   * 
   * @example
   * // Using session key (faster, no WebAuthn prompt)
   * await transfer.send(auth.sessionSigner, {
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc",
   *   value: 100_000_000_000n // 0.1 KSM
   * });
   */
  async send(
    sessionSigner: any | null,
    options: TransferOptions,
  ): Promise<TransferResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      
      const transferTx = kreivoApi.tx.Balances.transfer_keep_alive({
        dest: MultiAddress.Id(options.dest),
        value: options.value,
      });

      const txRes = await transferTx.signAndSubmit(sessionSigner);

      console.log("Transfer result:", txRes);

      return {
        hash: txRes.txHash,
        blockHash: txRes.block?.hash,
        success: true
      };
    } catch (error) {
      console.error("Transfer failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Transfer a specific amount to a destination address by username
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Transfer options including username and amount
   * @returns Promise with the transaction result
   * 
   * @example
   * // Using session key (faster, no WebAuthn prompt)
   * await transfer.sendByUsername(auth.sessionSigner, {
   *   username: "alice",
   *   value: 100_000_000_000n // 0.1 KSM
   * });
   */
  async sendByUsername(
    sessionSigner: any | null,
    options: TransferByUsernameOptions,
  ): Promise<TransferResult> {
    try {
      const destAddress = await this.resolveUsernameToAddress(options.username);
      
      return await this.send(
        sessionSigner,
        {
          dest: destAddress,
          value: options.value,
        },
      );
    } catch (error) {
      console.error("Transfer by username failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Transfer all available balance to a destination address
   * 
   * @param sessionSigner - Optional session signer for faster transactions  
   * @param options - SendAll options including destination
   * @returns Promise with the transaction result
   * 
   * @example
   * await transfer.sendAll(auth.sessionSigner, {
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc"
   * });
   */
  async sendAll(
    sessionSigner: any | null,
    options: SendAllOptions,
  ): Promise<TransferResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      
      const transferMethod = options.keepAlive !== false 
        ? kreivoApi.tx.Balances.transfer_all 
        : kreivoApi.tx.Balances.transfer_all;

      const transferTx = transferMethod({
        dest: MultiAddress.Id(options.dest),
        keep_alive: options.keepAlive !== false,
      });

      const txRes = await transferTx.signAndSubmit(sessionSigner);

      console.log("SendAll result:", txRes);

      return {
        hash: txRes.txHash,
        blockHash: txRes.block?.hash,
        success: true
      };
    } catch (error) {
      console.error("SendAll failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Transfer all available balance to a destination address by username
   * 
   * @param sessionSigner - Optional session signer for faster transactions  
   * @param options - SendAll options including username
   * @returns Promise with the transaction result
   * 
   * @example
   * await transfer.sendAllByUsername(auth.sessionSigner, {
   *   username: "alice"
   * });
   */
  async sendAllByUsername(
    sessionSigner: any | null,
    options: SendAllByUsernameOptions,
  ): Promise<TransferResult> {
    try {
      const destAddress = await this.resolveUsernameToAddress(options.username);
      
      return await this.sendAll(
        sessionSigner,
        {
          dest: destAddress,
          keepAlive: options.keepAlive,
        },
      );
    } catch (error) {
      console.error("SendAll by username failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Get balance information for an address
   * 
   * @param address - The SS58 address to query
   * @returns Promise with detailed balance information
   * 
   * @example
   * const balance = await transfer.getBalance("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc");
   * console.log(`Free balance: ${transfer.formatAmount(balance.free)} KSM`);
   */
  async getBalance(address: string): Promise<BalanceInfo> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    const accountInfo = await kreivoApi.query.System.Account.getValue(address);
    
    const { free, reserved, frozen } = accountInfo.data;
    const total = free + reserved;
    const transferable = free - frozen;

    return {
      free,
      reserved, 
      frozen,
      total,
      transferable: transferable > 0n ? transferable : 0n
    };
  }

  /**
   * Get balance information for a user by username
   * 
   * @param username - The username to query
   * @returns Promise with detailed balance information
   * 
   * @example
   * const balance = await transfer.getBalanceByUsername("alice");
   * console.log(`Alice's balance: ${transfer.formatAmount(balance.free)} KSM`);
   */
  async getBalanceByUsername(username: string): Promise<BalanceInfo> {
    const address = await this.resolveUsernameToAddress(username);
    return await this.getBalance(address);
  }

  /**
   * Get user information by username
   * 
   * @param username - The username to resolve
   * @returns Promise with user information including address
   * 
   * @example
   * const userInfo = await transfer.getUserInfo("alice");
   * console.log(`Alice's address: ${userInfo.address}`);
   */
  async getUserInfo(username: string): Promise<UserInfo> {
    const address = await this.resolveUsernameToAddress(username);
    return {
      address,
      username,
    };
  }

  /**
   * Format an amount from units to KSM with decimal places
   * 
   * @param amount - Amount in units (smallest unit)
   * @param decimals - Number of decimal places (default: 12 for Kusama)
   * @returns Formatted string with KSM amount
   * 
   * @example
   * transfer.formatAmount(1_000_000_000_000n) // "1.000000000000"
   * transfer.formatAmount(500_000_000_000n)   // "0.500000000000"
   */
  formatAmount(amount: bigint, decimals: number = 12): string {
    const divisor = 10n ** BigInt(decimals);
    const whole = amount / divisor;
    const remainder = amount % divisor;
    
    const remainderStr = remainder.toString().padStart(decimals, '0');
    return `${whole}.${remainderStr}`;
  }

  /**
   * Parse an amount from KSM string to units
   * 
   * @param amount - Amount as string (e.g., "1.5" for 1.5 KSM)
   * @param decimals - Number of decimal places (default: 12 for Kusama)
   * @returns Amount in units
   * 
   * @example
   * transfer.parseAmount("1.5")     // 1_500_000_000_000n
   * transfer.parseAmount("0.001")   // 1_000_000_000n
   */
  parseAmount(amount: string, decimals: number = 12): bigint {
    console.log("parseAmount", amount);
    const [whole = "0", decimal = ""] = amount.split(".");
    const paddedDecimal = decimal.padEnd(decimals, "0").slice(0, decimals);
    
    const wholeAmount = BigInt(whole) * (10n ** BigInt(decimals));
    const decimalAmount = BigInt(paddedDecimal);
    
    return wholeAmount + decimalAmount;
  }

  /**
   * Validate if an address is a valid SS58 address
   * 
   * @param address - The address string to validate
   * @returns true if valid, false otherwise
   * 
   * @example
   * transfer.isValidAddress("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc") // true
   * transfer.isValidAddress("invalid") // false
   */
  isValidAddress(address: string): boolean {
    try {
      ss58Decode(address);
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Get the public key from a PasskeysAuthenticator as an SS58 address
   * 
   * @param passkeysAuthenticator - The PasskeysAuthenticator instance
   * @returns The SS58 encoded address
   * 
   * @example
   * const address = transfer.getAddressFromAuthenticator(auth.passkeysAuthenticator);
   * console.log("User address:", address);
   */
  getAddressFromAuthenticator(passkeysAuthenticator: PasskeysAuthenticator): string {
    const passSigner = new KreivoPassSigner(passkeysAuthenticator);
    return ss58Encode(passSigner.publicKey);
  }

  /**
   * Creates an unsigned transfer extrinsic for batch operations
   * 
   * @param options - Transfer options including destination and amount
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const transferExtrinsic = await transfer.createTransferExtrinsic({
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc",
   *   value: 100_000_000_000n
   * });
   */
  async createTransferExtrinsic(options: TransferOptions): Promise<TransferExtrinsic> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    return kreivoApi.tx.Balances.transfer_keep_alive({
      dest: MultiAddress.Id(options.dest),
      value: options.value,
    });
  }

  /**
   * Creates an unsigned transfer extrinsic by username for batch operations
   * 
   * @param options - Transfer options including username and amount
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const transferExtrinsic = await transfer.createTransferByUsernameExtrinsic({
   *   username: "alice",
   *   value: 100_000_000_000n
   * });
   */
  async createTransferByUsernameExtrinsic(options: TransferByUsernameOptions): Promise<TransferExtrinsic> {
    const destAddress = await this.resolveUsernameToAddress(options.username);
    
    return this.createTransferExtrinsic({
      dest: destAddress,
      value: options.value,
    });
  }

  /**
   * Creates an unsigned send all extrinsic for batch operations
   * 
   * @param options - SendAll options including destination
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const sendAllExtrinsic = await transfer.createSendAllExtrinsic({
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc"
   * });
   */
  async createSendAllExtrinsic(options: SendAllOptions): Promise<TransferExtrinsic> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    return kreivoApi.tx.Balances.transfer_all({
      dest: MultiAddress.Id(options.dest),
      keep_alive: options.keepAlive !== false,
    });
  }

  /**
   * Creates an unsigned send all extrinsic by username for batch operations
   * 
   * @param options - SendAll options including username
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const sendAllExtrinsic = await transfer.createSendAllByUsernameExtrinsic({
   *   username: "alice"
   * });
   */
  async createSendAllByUsernameExtrinsic(options: SendAllByUsernameOptions): Promise<TransferExtrinsic> {
    const destAddress = await this.resolveUsernameToAddress(options.username);
    
    return this.createSendAllExtrinsic({
      dest: destAddress,
      keepAlive: options.keepAlive,
    });
  }
} 