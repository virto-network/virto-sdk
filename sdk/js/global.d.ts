import 'jest-environment-puppeteer';
import { SubeOptions } from '@virtonetwork/sube';
declare global {
  const page: import('puppeteer').Page;
  const browser: import('puppeteer').Browser;
  const jestPuppeteer: import('jest-puppeteer').Global;
}

import { SDK } from "./src/sdk";

declare global {
  interface Window {
    SDK: typeof SDK;
    WalletType: typeof WalletType;
    mockSube<T>(url: string, options?: SubeOptions): Promise<T>;
    jsWalletFn(mnemonic?: string): any;
  }
}

