import { kreivo } from "@polkadot-api/descriptors";
import { Binary, PolkadotClient } from "polkadot-api";
import TransactionQueue from "./transactionQueue";
import { TransactionResult } from "./types";

export interface RemarkOptions {
  remark: string;
}

export default class System {
  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>,
    private readonly transactionQueue?: TransactionQueue
  ) {}

  private async getClient(): Promise<PolkadotClient> {
    return await this.clientFactory();
  }

  /**
   * Make a remark with event (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Remark options including the remark string
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await system.makeRemarkAsync(auth.sessionSigner, {
   *   remark: "Hello, World!"
   * });
   * console.log("Transaction included:", result.hash);
   */
  async makeRemarkAsync(
    sessionSigner: any | null,
    options: RemarkOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for makeRemarkAsync");
      }
      
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.System.remark_with_event({
        remark: Binary.fromText(options.remark),
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
   * Make a simple remark (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Remark options including the remark string
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await system.makeSimpleRemarkAsync(auth.sessionSigner, {
   *   remark: "Hello, World!"
   * });
   * console.log("Transaction included:", result.hash);
   */
  async makeSimpleRemarkAsync(
    sessionSigner: any | null,
    options: RemarkOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for makeSimpleRemarkAsync");
      }
      
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.System.remark({
        remark: Binary.fromText(options.remark),
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
   * Creates an unsigned remark extrinsic for batch operations
   * 
   * @param options - Remark options including the remark string
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const remarkExtrinsic = await system.createRemarkExtrinsic({
   *   remark: "Hello, World!"
   * });
   */
  async createRemarkExtrinsic(options: RemarkOptions): Promise<any> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    const extrinsic = kreivoApi.tx.System.remark_with_event({
      remark: Binary.fromText(options.remark),
    });

    return extrinsic;
  }

  /**
   * Creates an unsigned simple remark extrinsic for batch operations
   * 
   * @param options - Remark options including the remark string
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const remarkExtrinsic = await system.createSimpleRemarkExtrinsic({
   *   remark: "Hello, World!"
   * });
   */
  async createSimpleRemarkExtrinsic(options: RemarkOptions): Promise<any> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    const extrinsic = kreivoApi.tx.System.remark({
      remark: Binary.fromText(options.remark),
    });

    return extrinsic;
  }
} 