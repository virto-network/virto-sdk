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

export interface TransactionResult {
  ok: boolean;
  hash?: string;
  error?: string;
}

export interface AttestationData {
  authenticator_data: string;
  client_data: string;
  public_key: string;
  meta: {
    deviceId: string;
    context: number;
    authority_id: string;
  }
}

export interface PreparedRegistrationData {
  attestation: AttestationData;
  hashedUserId: string;
  credentialId: string;
  userId: string;
  passAccountAddress: string;
}

export interface ServerSDKOptions {
  federate_server: string;
  provider_url: string;
  config: {
      jwt: {
          secret: string;
          expiresIn?: string;
      }
  };
}

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
  publicKey: string;
  address: string;
  exp: number;
  iat: number;
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
  onProviderStatusChange?: (status: any) => void;
}

export type SignFn = (input: Uint8Array) => Promise<Uint8Array> | Uint8Array;
