import { throws, deepEqual } from "node:assert";
import { describe, it } from "node:test";
import { Wallet } from "libwallet-js";

describe("Wallet", () => {
  const phrase =
    "myself web subject call unfair return skull fatal radio spray insect fall twist ladder audit jump gravity modify search only blouse review receive south";

  describe("constructor", () => {
    it("initializes a wallet using a randomly-generated seed", () => {
      throws(() => new Wallet());
      throws(() => new Wallet({}));

      new Wallet({ Simple: undefined });
      new Wallet({ Simple: null });
    });

    it("initializes a wallet using a given seed", () => {
      new Wallet({ Simple: phrase });
    });
  });

  describe(".address", () => {
    it("fails to retrieve if wallet is unlocked", () => {
      const wallet = new Wallet({ Simple: phrase });
      throws(() => wallet.address);
    });

    it("fails to retrieve if wallet is unlocked", async () => {
      const wallet = new Wallet({ Simple: phrase });
      await wallet.unlock();
      deepEqual(
        [...wallet.address],
        [
          108, 204, 206, 223, 179, 1, 220, 225, 205, 117, 149, 151, 188, 225,
          113, 10, 136, 122, 112, 31, 72, 132, 118, 58, 116, 31, 226, 197, 27,
          238, 54, 17,
        ]
      );
    });
  });
});
