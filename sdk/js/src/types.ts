export type BaseProfile = {
    id: string;
    name?: string;
};

export type User<Profile> = {
    profile: Profile;
};

export type Command = {
    url: string;
    body: any;
    hex: string;
};

export enum WalletType {
  VIRTO = "virto",
  POLKADOT = "polkadot"
}

export interface TransactionResult {
  ok: boolean;
  hash?: string;
  error?: string;
}

export type { 
  TransactionStatus, 
  TransactionMetadata, 
  TransactionEvent, 
  TransactionEventCallback,
  TransactionEventType,
} from './transactionQueue';

export type TransactionConfirmationLevel = 'submitted' | 'included' | 'finalized';

export interface SDKOptions {
  federate_server: string;
  provider_url: string;
  confirmation_level?: TransactionConfirmationLevel;
}
