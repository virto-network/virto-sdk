import Auth from '../../src/auth';
import SDK from '../../src/sdk';
import { sube } from '@virtonetwork/sube';
import { JsWallet } from '@virtonetwork/libwallet';

(async () => {
  const sdk = new SDK({
    federate_server: "http://localhost:3000",
    config: {
      wallet: "polkadotjs"
    }
  });
  const hashedUserId = new Uint8Array(
    await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode("testuser@example.com")
    )
  );

  console.log("hashedUserId", hashedUserId);

  document.getElementById('registerButton').addEventListener('click', async () => {
    const userName = document.getElementById('userName').value;
    const userDisplayName = document.getElementById('userDisplayName').value;

    const user = {
      profile: {
        id: hashedUserId,
        name: userName,
        displayName: userDisplayName,
      },
      metadata: { role: "admin" },
    };

    try {
      const result = await sdk.auth.register(user);
      console.log('Registration successful:', result);
    } catch (error) {
      console.error('Registration failed:', error);
    }
  });

  document.getElementById('connectButton').addEventListener('click', async () => {
    try {
      const result = await sdk.auth.connect(hashedUserId);
      console.log('Connection successful:', result);
    } catch (error) {
      console.error('Connection failed:', error);
    }
  });

  document.getElementById('signButton').addEventListener('click', async () => {
    const command = JSON.parse(document.getElementById('command').value);

    try {
      const result = await sdk.auth.sign(hashedUserId, command);
      console.log('Signing successful:', result);
    } catch (error) {
      console.error('Signing failed:', error);
    }
  });
})(); 