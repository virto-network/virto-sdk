import SDK from '../../src/sdk';

(async () => {
  const sdk = new SDK({
    // Should be a VOS implementation
    federate_server: "http://localhost:3000",
    // Should be a local, test or production url
    provider_url: "ws://localhost:12281",
    config: {
      wallet: "polkadotjs"
    }
  });

  document.getElementById('registerButton').addEventListener('click', async () => {
    const userName = document.getElementById('userName').value;
    const userDisplayName = document.getElementById('userDisplayName').value;

    const user = {
      profile: {
        id: "testuser@example.com",
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
      const result = await sdk.auth.connect("testuser@example.com");
      console.log('Connection successful:', result);
    } catch (error) {
      console.error('Connection failed:', error);
    }
  });

  document.getElementById('signButton').addEventListener('click', async () => {
    const command = JSON.parse(document.getElementById('command').value);

    try {
      const result = await sdk.auth.sign("testuser@example.com", command);
      console.log('Signing successful:', result);
    } catch (error) {
      console.error('Signing failed:', error);
    }
  });
})(); 