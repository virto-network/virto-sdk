import { kreivo, MultiAddress } from "@virtonetwork/sdk/descriptors";
import { PolkadotClient } from "polkadot-api";
import { ss58Decode } from "@polkadot-labs/hdkd-helpers";
import TransactionQueue from "./transactionQueue";
import { TransactionResult } from "./types";
import { UserService } from "./services/userService";

export interface TransferOptions {
  dest: string;
  value: bigint;
}

export interface TransferByUsernameOptions {
  username: string;
  value: bigint;
}

export interface SendAllOptions {
  dest: string;
  keepAlive?: boolean;
}

export interface SendAllByUsernameOptions {
  username: string;
  keepAlive?: boolean;
}

export interface BalanceInfo {
  free: bigint;
  reserved: bigint;
  frozen: bigint;
  total: bigint;
  transferable: bigint;
}

export interface UserInfo {
  address: string;
  username: string;
}

export type TransferExtrinsic = any;

export default class Transfer {
  private userService: UserService | null = null;

  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>,
    userService?: UserService,
    private readonly transactionQueue?: TransactionQueue
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
   * Transfer a specific amount to a destination address (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Transfer options including destination and amount
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await transfer.sendAsync(auth.sessionSigner, {
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc",
   *   value: 100_000_000_000n // 0.1 KSM
   * });
   * console.log("Transaction included:", result.hash);
   */
  async sendAsync(
    sessionSigner: any | null,
    options: TransferOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for sendAsync");
      }
      
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.Balances.transfer_keep_alive({
        dest: MultiAddress.Id(options.dest),
        value: options.value,
      });
      
      const transactionId = await this.transactionQueue.addTransaction(
        transaction,
        sessionSigner
      );

      const result = await this.transactionQueue.executeTransaction(transactionId, sessionSigner);

      return {
        ok: result.included,
        hash: result.hash,
        error: result.error
      };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Transfer a specific amount to a destination address by username (WAITS FOR INCLUSION)
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Transfer options including username and amount
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await transfer.sendByUsernameAsync(auth.sessionSigner, {
   *   username: "alice",
   *   value: 100_000_000_000n // 0.1 KSM
   * });
   */
  async sendByUsernameAsync(
    sessionSigner: any | null,
    options: TransferByUsernameOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for sendByUsernameAsync");
      }
      
      const destAddress = await this.resolveUsernameToAddress(options.username);
      
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.Balances.transfer_keep_alive({
        dest: MultiAddress.Id(destAddress),
        value: options.value,
      });
      
      const transactionId = await this.transactionQueue.addTransaction(
        transaction,
        sessionSigner
      );

      const result = await this.transactionQueue.executeTransaction(transactionId, sessionSigner);

      return {
        ok: result.included,
        hash: result.hash,
        error: result.error
      };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Transfer all available balance to a destination address (WAITS FOR INCLUSION)
   * 
   * @param sessionSigner - Optional session signer for faster transactions  
   * @param options - SendAll options including destination
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await transfer.sendAllAsync(auth.sessionSigner, {
   *   dest: "5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc"
   * });
   * console.log("Transaction included:", result.hash);
   */
  async sendAllAsync(
    sessionSigner: any | null,
    options: SendAllOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for sendAllAsync");
      }
      
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.Balances.transfer_all({
        dest: MultiAddress.Id(options.dest),
        keep_alive: options.keepAlive !== false,
      });
      
      const transactionId = await this.transactionQueue.addTransaction(
        transaction,
        sessionSigner
      );

      const result = await this.transactionQueue.executeTransaction(transactionId, sessionSigner);

      return {
        ok: result.included,
        hash: result.hash,
        error: result.error
      };
    } catch (error) {
      return {
        ok: false,
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
    const transferable: bigint = free - frozen;

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
    
    const extrinsic = kreivoApi.tx.Balances.transfer_keep_alive({
      dest: MultiAddress.Id(options.dest),
      value: options.value,
    });

    return extrinsic;
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
    
    const extrinsic = kreivoApi.tx.Balances.transfer_all({
      dest: MultiAddress.Id(options.dest),
      keep_alive: options.keepAlive !== false,
    });

    return extrinsic;
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