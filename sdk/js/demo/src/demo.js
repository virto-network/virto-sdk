import SDK from '../../src/sdk';
// import SDK from "../../dist/esm/sdk.js";
import { IndexedDBStorage } from "./storage/IndexedDBStorage";

(async () => {
  const storage = new IndexedDBStorage('VirtoSessions', 'sessions');
  const sdk = new SDK({
    // Should be a VOS implementation
    federate_server: "http://localhost:3001/api",
    // Should be a local, test or production url
    provider_url: "ws://localhost:22000",
    
  }, 
  storage);

  document.getElementById('registerButton').addEventListener('click', async () => {
    const userName = document.getElementById('userName').value;
    const userDisplayName = document.getElementById('userDisplayName').value;

    const user = {
      profile: {
        id: userName,
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
      const userName = document.getElementById('userName').value;
      const result = await sdk.auth.connect(userName);
      console.log('Connection successful:', result);
    } catch (error) {
      console.error('Connection failed:', error);
    }
  });

  document.getElementById('signButton').addEventListener('click', async () => {
    const command = JSON.parse(document.getElementById('command').value);

    try {
      const result = await sdk.auth.makeRemark(command);
      console.log('Signing successful:', result);
    } catch (error) {
      console.error('Signing failed:', error);
    }
  });

  document.getElementById('transferButton').addEventListener('click', async () => {
    const destAddress = document.getElementById('transferDest').value;
    const amount = document.getElementById('transferAmount').value;
    const useSessionKey = document.getElementById('useSessionKey').checked;

    try {
      if (!sdk.transfer.isValidAddress(destAddress)) {
        alert('Invalid destination address format');
        return;
      }

      if (!sdk.auth.passkeysAuthenticator) {
        alert('Please register first');
        return;
      }

      const amountInUnits = sdk.transfer.parseAmount(amount);
      console.log(`Sending ${amount} (${amountInUnits} units) to ${destAddress}`);

      const userAddress = sdk.transfer.getAddressFromAuthenticator(sdk.auth.passkeysAuthenticator);
      const balance = await sdk.transfer.getBalance(userAddress);
      console.log(`Current balance: ${sdk.transfer.formatAmount(balance.transferable)} KSM transferable`);

      // Execute transfer
      const result = await sdk.transfer.send(
        sdk.auth.passkeysAuthenticator,
        useSessionKey ? sdk.auth.sessionSigner : null,
        {
          dest: destAddress,
          value: amountInUnits
        },
        useSessionKey
      );

      if (result.success) {
        console.log('Transfer successful:', result);
        alert(`Transfer successful!\nTransaction Hash: ${result.hash}`);
      } else {
        console.error('Transfer failed:', result.error);
        alert(`Transfer failed: ${result.error}`);
      }

    } catch (error) {
      console.error('Transfer failed:', error);
      alert(`Transfer failed: ${error.message}`);
    }
  });

  document.getElementById('balanceButton').addEventListener('click', async () => {
    try {
      if (!sdk.auth.passkeysAuthenticator) {
        alert('Please register first');
        return;
      }

      const userAddress = sdk.transfer.getAddressFromAuthenticator(sdk.auth.passkeysAuthenticator);
      console.log(`Checking balance for address: ${userAddress}`);
      
      const balance = await sdk.transfer.getBalance(userAddress);

      console.log('Balance Info:', balance);
      alert(balance);

    } catch (error) {
      console.error('Balance check failed:', error);
      alert(`Balance check failed: ${error.message}`);
    }
  });

  document.getElementById('batchButton').addEventListener('click', async () => {
    const destAddress = document.getElementById('batchDest').value;
    const amount = document.getElementById('batchAmount').value;
    const message = document.getElementById('batchMessage').value;
    const useSessionKey = document.getElementById('useBatchSessionKey').checked;

    try {
      if (!sdk.transfer.isValidAddress(destAddress)) {
        alert('Invalid destination address format');
        return;
      }

      if (!sdk.auth.passkeysAuthenticator) {
        alert('Please register first');
        return;
      }

      const amountInUnits = sdk.transfer.parseAmount(amount);

      const userAddress = sdk.transfer.getAddressFromAuthenticator(sdk.auth.passkeysAuthenticator);
      const balance = await sdk.transfer.getBalance(userAddress);

      console.log('Creating extrinsics...');
      
      // Create transfer extrinsic
      const transferExtrinsic = await sdk.transfer.createTransferExtrinsic({
        dest: destAddress,
        value: amountInUnits
      });
      console.log('Transfer extrinsic created');

      // Create remark extrinsic
      const remarkExtrinsic = await sdk.system.createRemarkExtrinsic({
        message: message
      });
      console.log('Remark extrinsic created');

      console.log('Executing batch...');
      
      // Execute batch
      const result = await sdk.utility.batchAll(
        sdk.auth.passkeysAuthenticator,
        useSessionKey ? sdk.auth.sessionSigner : null,
        {
          calls: [transferExtrinsic, remarkExtrinsic]
        },
        !useSessionKey // useMainKey
      );

      if (result.success) {
        console.log('Batch successful:', result);
        alert(`Batch successful!\nTransaction Hash: ${result.hash}\n\nBoth transfer (${amount} KSM to ${destAddress.slice(0, 10)}...) and remark ("${message}") were executed atomically.`);
      } else {
        console.error('Batch failed:', result.error);
        alert(`Batch failed: ${result.error}`);
      }

    } catch (error) {
      console.error('Batch execution failed:', error);
      alert(`Batch execution failed: ${error.message}`);
    }
  });
})(); 