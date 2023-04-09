const { JsWallet, JsPublicAddress } = require("../pkg/libwallet_js.js");

class Wallet extends JsWallet {
  /** @type {PublicAddress} */
  #address;

  constructor(ctor) {
    super(
      ctor ?? {
        Simple: null,
      }
    );

    Object.defineProperty(this, "address", {
      get: () => {
        this.#address ??= new PublicAddress(super.getAddress());
        return this.#address;
      },
    });
  }
};

class PublicAddress {
  /** @type {JsPublicAddress} */
  #publicAddress;

  [Symbol.iterator]() {
    console.log("PublicAddress@@iterator");
    return this.#publicAddress.repr[Symbol.iterator]();
  }

  constructor(jsPublicAddress) {
    this.#publicAddress = jsPublicAddress;
  }

  toHex() {
    try {
      return this.#publicAddress.toHex();
    } catch (error) {
      throw new Error("Not implemented", { cause: error });
    }
  }
};

exports.Wallet = Wallet;
exports.PublicAddress = PublicAddress;