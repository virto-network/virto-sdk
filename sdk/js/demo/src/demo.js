// import SDK from '../../src/sdk';
import SDK from "../../dist/esm/index.js";
import { IndexedDBStorage } from "./storage/IndexedDBStorage";
import { 
  createEd25519Signer, 
  storeMnemonic, 
  getMnemonic, 
  hasMnemonic 
} from "./signerUtils";

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

  let currentSubstrateSigner = null;

  function getAuthenticatorType() {
    return document.getElementById('authenticatorType').value;
  }

  document.getElementById('authenticatorType').addEventListener('change', (e) => {
    const infoElement = document.getElementById('authenticatorInfo');
    if (e.target.value === 'webauthn') {
      infoElement.textContent = 'Using WebAuthn for biometric/hardware authentication';
    } else {
      infoElement.textContent = 'Using Substrate Key (ed25519) - A mnemonic will be generated and stored locally';
    }
  });

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
    const authType = getAuthenticatorType();

    const user = {
      profile: {
        id: userName,
        name: userName,
        displayName: userDisplayName,
      },
      metadata: { role: "admin" },
    };

    try {
      let result;
      
      if (authType === 'webauthn') {
        result = await sdk.auth.register(user);
        console.log('WebAuthn Registration successful:', result);
      } else {
        const { signer, mnemonic } = createEd25519Signer();
        currentSubstrateSigner = signer;
        
        storeMnemonic(userName, mnemonic);
        
        console.log('Generated mnemonic for', userName);
        console.log('Mnemonic (save this!):', mnemonic);
        alert(`Substrate Key Generated!\n\nMnemonic (SAVE THIS):\n${mnemonic}\n\nThis has been stored in localStorage for this demo.`);
        
        result = await sdk.auth.registerWithSubstrateKey(user, signer);
        console.log('Substrate Key Registration successful:', result);
      }
      
      const membershipResult = await sdk.auth.addMember(user.profile.id);
      console.log('Membership successful:', membershipResult);
    } catch (error) {
      console.error('Registration failed:', error);
      alert(`Registration failed: ${error.message}`);
    }
  });

  document.getElementById('connectButton').addEventListener('click', async () => {
    try {
      const userName = document.getElementById('userName').value;
      const authType = getAuthenticatorType();
      
      console.log('Connecting user:', userName, 'with', authType);
      
      let result;
      
      if (authType === 'webauthn') {
        result = await sdk.auth.connect(userName);
        console.log('WebAuthn Connection successful:', result);
      } else {
        const mnemonic = getMnemonic(userName);
        
        if (!mnemonic) {
          alert(`No substrate key found for user ${userName}. Please register first.`);
          console.error('No mnemonic found for user:', userName);
          return;
        }
        
        const { signer } = createEd25519Signer(mnemonic);
        currentSubstrateSigner = signer;
        
        result = await sdk.auth.connectWithSubstrateKey(userName, signer);
        console.log('Substrate Key Connection successful:', result);
      }
      
      console.log('Connection result:', result);
    } catch (error) {
      console.error('Connection failed:', error);
      alert(`Connection failed: ${error.message}`);
    }
  });


  document.getElementById('signButton').addEventListener('click', async () => {
    const command = JSON.parse(document.getElementById('command').value);

    try {
      const hasAuth = sdk.auth.isAuthenticated();
      if (!hasAuth) {
        alert('Please register and connect first');
        console.log('Please register and connect first');
        return;
      }

      if (!sdk.auth.sessionSigner) {
        alert('No session signer available. Please connect first.');
        console.log('No session signer available');
        return;
      }

      disableButtons();

      const result = await sdk.system.makeRemarkAsync(sdk.auth.sessionSigner, {
        remark: command.remark || "Hello, World!"
      });
      console.log('Signing successful:', result);
    } catch (error) {
      console.error('Signing failed:', error);
      alert(`Signing failed: ${error.message}`);
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
        alert('Invalid destination address format');
        console.log('Invalid destination address format');
        return;
      }

      const hasAuth = sdk.auth.isAuthenticated();
      if (!hasAuth) {
        alert('Please register and connect first');
        console.log('Please register and connect first');
        return;
      }

      disableButtons();

      const amountInUnits = sdk.transfer.parseAmount(amount);
      console.log(`Sending ${amount} (${amountInUnits} units) to ${destAddress}`);

      const result = await sdk.transfer.sendAsync(
        useSessionKey ? sdk.auth.sessionSigner : sdk.auth.currentAuthenticator,
        {
          dest: destAddress,
          value: amountInUnits
        }
      );

      if (result.ok) {
        console.log('Transfer ready:', result);
      } else {
        console.error('Transfer failed:', result.error);
        alert(`Transfer failed: ${result.error}`);
      }

      enableButtons();
    } catch (error) {
      console.error('Transfer failed:', error);
      alert(`Transfer failed: ${error.message}`);
      enableButtons();
    }
  });

  document.getElementById('balanceButton').addEventListener('click', async () => {
    try {
      const hasAuth = sdk.auth.isAuthenticated();
      if (!hasAuth) {
        alert('Please register and connect first');
        console.log('Please register and connect first');
        return;
      }

      const authenticator = sdk.auth.currentAuthenticator || 
                           sdk.auth.passkeysAuthenticator || 
                           sdk.auth.substrateKeyAuthenticator;
      
      if (!authenticator) {
        alert('No authenticator available');
        return;
      }

      const userAddress = sdk.auth.getAddressFromAuthenticator(authenticator);
      console.log(`Checking balance for address: ${userAddress}`);

      const balance = await sdk.transfer.getBalance(userAddress);

      console.log('Balance Info:', balance);
      alert(`Balance for ${userAddress}:\n\nFree: ${balance.data.free}\nReserved: ${balance.data.reserved}\nFrozen: ${balance.data.frozen}`);

    } catch (error) {
      console.error('Balance check failed:', error);
      alert(`Balance check failed: ${error.message}`);
    }
  });

  document.getElementById('batchButton').addEventListener('click', async () => {
    const destAddress = document.getElementById('batchDest').value;
    const amount = document.getElementById('batchAmount').value;
    const message = document.getElementById('batchMessage').value;
    const useSessionKey = true;

    try {
      if (!sdk.transfer.isValidAddress(destAddress)) {
        alert('Invalid destination address format');
        console.log('Invalid destination address format');
        return;
      }

      const hasAuth = sdk.auth.isAuthenticated();
      if (!hasAuth) {
        alert('Please register and connect first');
        console.log('Please register and connect first');
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
        useSessionKey ? sdk.auth.sessionSigner : sdk.auth.currentAuthenticator,
        {
          calls: [transferExtrinsic, remarkExtrinsic]
        }
      );

      if (result.ok) {
        console.log('Batch ready:', result);
      } else {
        console.error('Batch failed:', result.error);
        alert(`Batch failed: ${result.error}`);
      }
      enableButtons();

    } catch (error) {
      console.error('Batch execution failed:', error);
      alert(`Batch execution failed: ${error.message}`);
      enableButtons();
    }
  });
})(); 