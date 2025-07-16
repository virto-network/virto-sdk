// Client SDK Exports (Browser)
export { default as SDK } from './sdk';
export { default as Auth } from './auth';

// Server SDK Exports (Node.js)
export { default as ServerSDK } from './serverSdk';
export { default as ServerAuth } from './serverAuth';

// Storage interfaces and implementations
export type { IStorage } from './storage';
export { LocalStorageImpl, InMemoryImpl } from './storage';

// Shared types
export type { PreparedCredentialData, PreparedRegistrationData } from './auth';
export type { PreparedConnectionData } from './serverAuth';
export type { Command, BaseProfile, User } from './types';
export { WalletType } from './types'; 