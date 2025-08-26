import SDK from '../../src/sdk';
// import SDK from "../../dist/esm/sdk.js";
import { IndexedDBStorage } from "./storage/IndexedDBStorage";

(async () => {
  const storage = new IndexedDBStorage('VirtoSessions', 'sessions');
  const sdk = new SDK({
    // Should be a VOS implementation
    federate_server: "http://localhost:3000/api",
    // Should be a local, test or production url
    provider_url: "ws://localhost:21000",
    confirmation_level: 'submitted',
    onProviderStatusChange: (status) => {
      switch (status.type) {
        case 0: // CONNECTING
          console.log(`Connecting to ${status.uri}...`);
          break;
        case 1: // CONNECTED
          console.log(`Connected to ${status.uri}!`);
          break;
        case 2: // ERROR
          console.log(`Connection error:`, status.event);
          break;
        case 3: // CLOSE
          console.log(`Connection closed`);
          break;
      }
    }
  }, storage);

  // Configure transaction event listeners
  sdk.onTransactionUpdate((event) => {
    if (event.type === 'included') {
      const blockHash = event.transaction.blockHash;
      const txHash = event.transaction.hash;
      console.log(`Transaction ${txHash} included in block: ${blockHash}`);
      enableButtons();
    }
    if (event.type === 'finalized') {
      console.log(`Transaction finalized: ${event.transaction.hash}`);
    }
    if (event.type === 'failed') {
      console.log(`Transaction failed: ${event.transaction.error}`);
      enableButtons();
    }

    updateTransactionHistory();
  });

  function disableButtons() {
    const transferButton = document.getElementById('transferButton');
    const batchButton = document.getElementById('batchButton');
    const signButton = document.getElementById('signButton');

    transferButton.disabled = true;
    batchButton.disabled = true;
    signButton.disabled = true;

    transferButton.classList.add('processing');
    batchButton.classList.add('processing');
    signButton.classList.add('processing');

    transferButton.textContent = 'Processing Transfer...';
    batchButton.textContent = 'Processing Batch...';
    signButton.textContent = 'Processing Sign...';
  }

  function enableButtons() {
    const transferButton = document.getElementById('transferButton');
    const batchButton = document.getElementById('batchButton');
    const signButton = document.getElementById('signButton');

    transferButton.disabled = false;
    batchButton.disabled = false;
    signButton.disabled = false;

    transferButton.classList.remove('processing');
    batchButton.classList.remove('processing');
    signButton.classList.remove('processing');

    transferButton.textContent = 'Send Transfer';
    batchButton.textContent = 'Execute Batch (Transfer + Remark)';
    signButton.textContent = 'Sign Message';
  }

  function updateTransactionHistory() {
    const transactionList = document.getElementById('transactionList');
    const history = sdk.getTransactionHistory();

    if (history.length === 0) {
      transactionList.innerHTML = '<p>No transactions yet...</p>';
      return;
    }

    transactionList.innerHTML = history.map(tx => {
      const time = new Date(tx.timestamp).toLocaleTimeString();
      const shortHash = tx.hash ? tx.hash.slice(0, 10) + '...' : 'pending';
      return `
        <div class="transaction-item tx-${tx.status}">
          <strong>Transaction ${tx.id}</strong> - 
          ${tx.status.toUpperCase()} - 
          ${time} - 
          Hash: ${shortHash}
          ${tx.error ? ` - Error: ${tx.error}` : ''}
        </div>
      `;
    }).join('');
  }

  document.getElementById('clearHistoryButton').addEventListener('click', () => {
    sdk.clearTransactionHistory();
    updateTransactionHistory();
  });

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
    disableButtons();
    const command = JSON.parse(document.getElementById('command').value);

    try {
      const result = await sdk.system.makeRemarkAsync(sdk.auth.sessionSigner, {
        remark: command.remark || "Hello, World!"
      });
      console.log('Signing successful:', result);
    } catch (error) {
      console.error('Signing failed:', error);
    } finally {
      enableButtons();
    }
  });

  document.getElementById('transferButton').addEventListener('click', async () => {
    const destAddress = document.getElementById('transferDest').value;
    const amount = document.getElementById('transferAmount').value;
    const useSessionKey = true;

    try {
      if (!sdk.transfer.isValidAddress(destAddress)) {
        console.log('Invalid destination address format');
        return;
      }

      if (!sdk.auth.passkeysAuthenticator) {
        console.log('Please register first');
        return;
      }

      disableButtons();

      const amountInUnits = sdk.transfer.parseAmount(amount);
      console.log(`Sending ${amount} (${amountInUnits} units) to ${destAddress}`);

      const result = await sdk.transfer.sendAsync(
        useSessionKey ? sdk.auth.sessionSigner : sdk.auth.passkeysAuthenticator,
        {
          dest: destAddress,
          value: amountInUnits
        }
      );

      if (result.ok) {
        console.log('Transfer ready:', result);
      } else {
        console.error('Transfer failed:', result.error);
        console.log(`Transfer failed: ${result.error}`);
      }

      enableButtons();
    } catch (error) {
      console.error('Transfer failed:', error);
      console.log(`Transfer failed: ${error.message}`);
      enableButtons();
    }
  });

  document.getElementById('balanceButton').addEventListener('click', async () => {
    try {
      if (!sdk.auth.passkeysAuthenticator) {
        console.log('Please register first');
        return;
      }

      const userAddress = sdk.auth.getAddressFromAuthenticator(sdk.auth.passkeysAuthenticator);
      console.log(`Checking balance for address: ${userAddress}`);

      const balance = await sdk.transfer.getBalance(userAddress);

      console.log('Balance Info:', balance);
      console.log(balance);

    } catch (error) {
      console.error('Balance check failed:', error);
      console.log(`Balance check failed: ${error.message}`);
    }
  });

  document.getElementById('batchButton').addEventListener('click', async () => {
    const destAddress = document.getElementById('batchDest').value;
    const amount = document.getElementById('batchAmount').value;
    const message = document.getElementById('batchMessage').value;
    const useSessionKey = true;

    try {
      if (!sdk.transfer.isValidAddress(destAddress)) {
        console.log('Invalid destination address format');
        return;
      }

      if (!sdk.auth.passkeysAuthenticator) {
        console.log('Please register first');
        return;
      }

      disableButtons();

      const amountInUnits = sdk.transfer.parseAmount(amount);

      const transferExtrinsic = await sdk.transfer.createTransferExtrinsic({
        dest: destAddress,
        value: amountInUnits
      });

      const remarkExtrinsic = await sdk.system.createRemarkExtrinsic({
        message: message
      });

      const result = await sdk.utility.batchAllAsync(
        useSessionKey ? sdk.auth.sessionSigner : null,
        {
          calls: [transferExtrinsic, remarkExtrinsic]
        }
      );

      if (result.ok) {
        console.log('Batch ready:', result);
      } else {
        console.error('Batch failed:', result.error);
        console.log(`Batch failed: ${result.error}`);
      }
      enableButtons();

    } catch (error) {
      console.error('Batch execution failed:', error);
      console.log(`Batch execution failed: ${error.message}`);
      enableButtons();
    }
  });
})(); 