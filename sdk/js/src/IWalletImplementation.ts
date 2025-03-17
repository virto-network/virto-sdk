import { Command } from "./types";

export interface IWalletImplementation {
  unlock(): Promise<void>;
  getAddress(): Promise<string>;
  sign(command: Command): Promise<boolean>;
  getMnemonic(): string;
}
