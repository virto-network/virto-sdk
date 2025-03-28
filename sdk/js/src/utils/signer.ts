import { ISubmittableResult } from "@polkadot/types/types";

import { KeyringPair } from "@polkadot/keyring/types";
import { SubmittableExtrinsic } from "@polkadot/api/types";

export const signSendAndWait = (
  tx: SubmittableExtrinsic<"promise">,
  signer: KeyringPair
) =>
  new Promise<ISubmittableResult>((resolve, reject) =>
    tx.signAndSend(signer, (result) => {
      console.debug("SIGN SEND CALLBACK", result.toHuman());
      switch (true) {
        case result.isError:
          return reject(result.status);
        case result.isFinalized:
          return resolve(result);
        case result.isWarning:
          console.warn(result.toHuman(true));
      }
    })
  );
