import { kreivo } from "@polkadot-api/descriptors";
import { PolkadotClient } from "polkadot-api";

export default class NonceManager {
  private nonceCache: Map<string, number> = new Map();
  private clientFactory: () => Promise<PolkadotClient>;
  private nonceLocks: Map<string, Promise<void>> = new Map();

  constructor(clientFactory: () => Promise<PolkadotClient>) {
    this.clientFactory = clientFactory;
  }

  /**
   * Get the current nonce for an address
   * 
   * @param address - The SS58 address to get nonce for
   * @returns Promise with the current nonce
   * 
   * @example
   * const nonce = await nonceManager.getNonce("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc");
   * console.log("Current nonce:", nonce);
   */
  async getNonce(address: string): Promise<number> {
    const client = await this.clientFactory();
    const kreivoApi = client.getTypedApi(kreivo);
    
    const accountInfo = await kreivoApi.query.System.Account.getValue(address);
    console.log("Account info getting Nonce:", accountInfo);
    return accountInfo.nonce;
  }

  /**
   * Get the cached nonce for an address or fetch it from the chain
   * 
   * @param address - The SS58 address to get nonce for
   * @returns Promise with the current nonce
   */
  async getCurrentNonce(address: string): Promise<number> {
    if (this.nonceCache.has(address)) {
      return this.nonceCache.get(address)!;
    }

    const nonce = await this.getNonce(address);
    this.nonceCache.set(address, nonce);
    return nonce;
  }

  /**
   * Get and increment nonce atomically (thread-safe)
   * This method ensures that no two transactions get the same nonce
   * 
   * @param address - The SS58 address to get and increment nonce for
   * @returns Promise with the nonce to use for the transaction
   */
  async getAndIncrementNonce(address: string): Promise<number> {
    if (!this.nonceLocks.has(address)) {
      this.nonceLocks.set(address, Promise.resolve());
    }

    await this.nonceLocks.get(address);

    let resolveLock: () => void;
    const lockPromise = new Promise<void>((resolve) => {
      resolveLock = resolve;
    });
    this.nonceLocks.set(address, lockPromise);

    try {
      const currentNonce = await this.getCurrentNonce(address);
      
      this.nonceCache.set(address, currentNonce + 1);
      
      return currentNonce;
    } finally {
      resolveLock!();
    }
  }

  /**
   * Increment the nonce for an address after a successful transaction
   * 
   * @param address - The SS58 address to increment nonce for
   */
  incrementNonce(address: string): void {
    const currentNonce = this.nonceCache.get(address) || 0;
    this.nonceCache.set(address, currentNonce + 1);
  }

  /**
   * Reset the nonce cache for an address (useful when transactions fail)
   * 
   * @param address - The SS58 address to reset nonce for
   */
  async resetNonce(address: string): Promise<void> {
    this.nonceCache.delete(address);
  }

  /**
   * Reset the nonce cache for all addresses
   */
  resetAllNonces(): void {
    this.nonceCache.clear();
  }

  /**
   * Get the cached nonce for an address (does not fetch from chain)
   * 
   * @param address - The SS58 address to get cached nonce for
   * @returns The cached nonce or undefined if not cached
   * 
   * @example
   * const cachedNonce = nonceManager.getCachedNonce("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc");
   * console.log("Cached nonce:", cachedNonce);
   */
  getCachedNonce(address: string): number | undefined {
    return this.nonceCache.get(address);
  }

  /**
   * Sync the cached nonce with the current nonce from the blockchain
   * This is useful when you want to ensure the cache is up to date
   * 
   * @param address - The SS58 address to sync nonce for
   * @returns Promise with the updated nonce
   * 
   * @example
   * const updatedNonce = await nonceManager.syncNonceFromChain("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc");
   * console.log("Synced nonce:", updatedNonce);
   */
  async syncNonceFromChain(address: string): Promise<number> {
    const blockchainNonce = await this.getNonce(address);
    this.nonceCache.set(address, blockchainNonce);
    return blockchainNonce;
  }

  /**
   * Get the current nonce from blockchain and update cache (alias for syncNonceFromChain)
   * 
   * @param address - The SS58 address to get nonce for
   * @returns Promise with the current nonce from blockchain
   * 
   * @example
   * const blockchainNonce = await nonceManager.getNonceFromChain("5Gq2VNFP4yqK1bk5zt552hBxU68Q3ABewGN99zY7qGbpVTFc");
   * console.log("Blockchain nonce:", blockchainNonce);
   */
  async getNonceFromChain(address: string): Promise<number> {
    return await this.syncNonceFromChain(address);
  }
} 