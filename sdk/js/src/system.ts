import { kreivo } from "@polkadot-api/descriptors";
import { Binary, PolkadotClient } from "polkadot-api";

export interface RemarkOptions {
  message: string;
}

export interface SystemResult {
  hash: string;
  blockHash?: string;
  success: boolean;
  error?: string;
}

// Type for unsigned extrinsics from system module
export type SystemExtrinsic = any;

export default class System {
  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>
  ) {}

  private async getClient(): Promise<PolkadotClient> {
    return await this.clientFactory();
  }

  /**
   * Makes a remark with event on the blockchain
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Remark options including the message
   * @returns Promise with the transaction result
   * 
   * @example
   * // Using session key (faster, no WebAuthn prompt)
   * await system.makeRemark(auth.sessionSigner, {
   *   message: "Hello world!"
   * });
   * 
   * // Using main key (secure, requires WebAuthn prompt)
   * await system.makeRemark(null, {
   *   message: "Hello world!"
   * });
   */
  async makeRemark(
    sessionSigner: any | null,
    options: RemarkOptions,
  ): Promise<SystemResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      
      const remarkTx = kreivoApi.tx.System.remark_with_event({
        remark: Binary.fromText(options.message),
      });

      let txRes;
      
      txRes = await remarkTx.signAndSubmit(sessionSigner);
      console.log("Remark:", txRes);

      return {
        hash: txRes.txHash,
        blockHash: txRes.block?.hash,
        success: true
      };
    } catch (error) {
      console.error("Remark failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Makes a simple remark (without event) on the blockchain
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Remark options including the message
   * @returns Promise with the transaction result
   * 
   * @example
   * await system.makeSimpleRemark(auth.sessionSigner, {
   *   message: "Simple remark"
   * });
   */
  async makeSimpleRemark(
    sessionSigner: any | null,
    options: RemarkOptions,
  ): Promise<SystemResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      
      const remarkTx = kreivoApi.tx.System.remark({
        remark: Binary.fromText(options.message),
      });

      const txRes = await remarkTx.signAndSubmit(sessionSigner);
      console.log("Simple remark:", txRes);

      return {
        hash: txRes.txHash,
        blockHash: txRes.block?.hash,
        success: true
      };
    } catch (error) {
      console.error("Simple remark failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /**
   * Creates an unsigned remark with event extrinsic for batch operations
   * 
   * @param options - Remark options including the message
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const remarkExtrinsic = await system.createRemarkExtrinsic({ message: "Hello world!" });
   */
  async createRemarkExtrinsic(options: RemarkOptions): Promise<SystemExtrinsic> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    return kreivoApi.tx.System.remark_with_event({
      remark: Binary.fromText(options.message),
    });
  }

  /**
   * Creates an unsigned simple remark extrinsic for batch operations
   * 
   * @param options - Remark options including the message
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const simpleRemarkExtrinsic = await system.createSimpleRemarkExtrinsic({ message: "Simple remark" });
   */
  async createSimpleRemarkExtrinsic(options: RemarkOptions): Promise<SystemExtrinsic> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);
    
    return kreivoApi.tx.System.remark({
      remark: Binary.fromText(options.message),
    });
  }

  /**
   * Validates if a message is appropriate for a remark
   * 
   * @param message - The message to validate
   * @returns true if valid, false otherwise
   * 
   * @example
   * system.isValidRemarkMessage("Hello world!") // true
   * system.isValidRemarkMessage("") // false
   */
  isValidRemarkMessage(message: string): boolean {
    if (!message || message.trim().length === 0) {
      return false;
    }

    // Check if message is not too long (arbitrary limit)
    if (message.length > 1000) {
      return false;
    }

    return true;
  }
} 