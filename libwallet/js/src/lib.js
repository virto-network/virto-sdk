import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { Wallet } = require("./wallet.cjs");

export { Wallet };
