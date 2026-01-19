import SDK from "./sdk";
import Auth from "./auth";
import Transfer from "./transfer";
import System from "./system";
import Utility from "./utility";
import CustomModule from "./custom";
import TransactionQueue from "./transactionQueue";
import TransactionExecutor from "./transactionExecutor";
import { DefaultUserService } from "./services/userService";
import ServerSDK from "./serverSdk";

export type {
  TransferOptions,
  TransferByUsernameOptions,
  SendAllOptions,
  SendAllByUsernameOptions,
  BalanceInfo,
  UserInfo,
} from "./transfer";

export type {
  RemarkOptions,
} from "./system";

export type {
  BatchOptions,
} from "./utility";

export type {
  TransactionStatus,
  TransactionMetadata,
  TransactionEventType,
  TransactionEvent,
  TransactionEventCallback,
} from "./transactionQueue";

export type { 
  SDKOptions, 
  TransactionConfirmationLevel,
  TransactionResult,
  AttestationData,
  PreparedRegistrationData,
  SubstrateKeyRegistrationData,
  PreparedSubstrateRegistrationData,
} from "./types";

export type {
  UserService,
} from "./services/userService";

export {
  SDK,
  ServerSDK,
  Auth,
  Transfer,
  System,
  Utility,
  CustomModule,
  TransactionQueue,
  TransactionExecutor,
  DefaultUserService,
};

export { SubstrateKey } from "@virtonetwork/authenticators-substrate";

export default SDK; 