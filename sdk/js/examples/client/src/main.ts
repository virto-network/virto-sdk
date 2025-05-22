
import { default as SDK } from '../../../dist/esm/sdk.js';
import { WalletType } from '../../../src/types';
import type { PreparedRegistrationData, PreparedConnectionData, Command } from '../../../src/index';

const JWT_TOKEN_KEY = 'virto_jwt_token';
const CONNECTED_USER_KEY = 'virto_connected_user';

function loadFromLocalStorage() {
  const storedToken = localStorage.getItem(JWT_TOKEN_KEY);
  const storedUserId = localStorage.getItem(CONNECTED_USER_KEY);

  if (storedToken) {
    authToken = storedToken;
    console.log('Token JWT loaded from localStorage', 'info');
  }

  if (storedUserId) {
    connectedUserId = storedUserId;
    console.log(`Connected user loaded from localStorage: ${storedUserId}`, 'info');
  }
}

let preparedRegistrationData: PreparedRegistrationData | null = null;
let preparedConnectionData: PreparedConnectionData | null = null;
let connectedUserId: string | null = null;
let authToken: string | null = null;

loadFromLocalStorage();

const userId = document.getElementById('userId') as HTMLInputElement;
const userName = document.getElementById('userName') as HTMLInputElement;
const commandHex = document.getElementById('commandHex') as HTMLInputElement;
const prepareRegistrationButton = document.getElementById('prepareRegistrationButton') as HTMLButtonElement;
const completeRegistrationButton = document.getElementById('completeRegistrationButton') as HTMLButtonElement;
const prepareConnectionButton = document.getElementById('prepareConnectionButton') as HTMLButtonElement;
const completeConnectionButton = document.getElementById('completeConnectionButton') as HTMLButtonElement;
const signButton = document.getElementById('signButton') as HTMLButtonElement;

function initializeSDK() {
  try {
    //@ts-ignore
    const sdk = new SDK({
      federate_server: 'http://localhost:3000/api',
      provider_url: 'ws://localhost:12281',
      config: {
        wallet: WalletType.POLKADOT
      }
    });

    console.log('SDK initialized successfully', 'success');
    return sdk;
  } catch (error) {
    console.log(`Error initializing SDK: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
    throw error;
  }
}

function getUserData() {
  return {
    profile: {
      id: userId.value,
      name: userName.value || undefined
    }
  };
}

function saveToLocalStorage() {
  if (authToken) {
    localStorage.setItem(JWT_TOKEN_KEY, authToken);
  }
  if (connectedUserId) {
    localStorage.setItem(CONNECTED_USER_KEY, connectedUserId);
  }
}

async function prepareRegistration() {
  try {
    console.log('Preparing registration data on the client...');

    if (!userId.value) {
      console.log('Error: User ID is required', 'error');
      return;
    }

    prepareRegistrationButton.disabled = true;

    const sdk = initializeSDK();
    const userData = getUserData();

    console.log(`Preparing registration for user with ID: ${userData.profile.id}...`);

    preparedRegistrationData = await sdk.auth.prepareRegistration(userData);

    console.log('Registration data prepared successfully:', 'success');
    console.log(JSON.stringify(preparedRegistrationData, null, 2));

    completeRegistrationButton.disabled = false;
  } catch (error) {
    console.log(`Error preparing registration: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
  } finally {
    prepareRegistrationButton.disabled = false;
  }
}

async function completeRegistration() {
  try {
    console.log('Sending data to the server to complete registration...');

    if (!preparedRegistrationData) {
      console.log('Error: No prepared registration data. Run Step 1 first.', 'error');
      return;
    }

    completeRegistrationButton.disabled = true;

    const response = await fetch(`http://localhost:9000/custom-register`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(preparedRegistrationData)
    });

    if (!response.ok) {
      throw new Error(`Server responded with status: ${response.status}`);
    }

    const result = await response.json();

    console.log('Registration completed successfully on the server:', 'success');
    console.log(JSON.stringify(result, null, 2));

    // After successful registration, automatically start the connection process
    const userId = preparedRegistrationData.userId;
    console.log(`Starting automatic connection process for ${userId}...`, 'info');

    preparedRegistrationData = null;
    completeRegistrationButton.disabled = true;

    await prepareConnection();

    // If we have prepared data, complete the connection
    if (preparedConnectionData) {
      await completeConnection();
    }
  } catch (error) {
    console.log(`Error completing registration: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
  } finally {
    completeRegistrationButton.disabled = !preparedRegistrationData;
  }
}

async function prepareConnection() {
  try {
    console.log('Preparing connection data on the client...');

    if (!userId.value) {
      console.log('Error: User ID is required', 'error');
      return;
    }

    prepareConnectionButton.disabled = true;

    const sdk = initializeSDK();

    console.log(`Preparing connection for user with ID: ${userId.value}...`);

    preparedConnectionData = await sdk.auth.prepareConnection(userId.value);

    console.log('Connection data prepared successfully:', 'success');
    console.log(JSON.stringify(preparedConnectionData, null, 2));

    completeConnectionButton.disabled = false;
  } catch (error) {
    console.log(`Error preparing connection: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
  } finally {
    prepareConnectionButton.disabled = false;
  }
}

async function completeConnection() {
  try {
    console.log('Sending data to the server to complete connection...');

    if (!preparedConnectionData) {
      console.log('Error: No prepared connection data. Run Step 1 first.', 'error');
      return;
    }

    completeConnectionButton.disabled = true;

    const response = await fetch(`http://localhost:9000/custom-connect`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(preparedConnectionData)
    });

    if (!response.ok) {
      throw new Error(`Server responded with status: ${response.status}`);
    }

    const result = await response.json();

    console.log('Connection completed successfully on the server:', 'success');
    console.log(JSON.stringify(result, null, 2));

    connectedUserId = preparedConnectionData.userId;

    authToken = result.token || null;

    if (authToken) {
      console.log('JWT token received and stored for future requests', 'success');
      saveToLocalStorage();
    } else {
      console.log('Warning: No JWT token received from server', 'warning');
    }

    preparedConnectionData = null;
    completeConnectionButton.disabled = true;
  } catch (error) {
    console.log(`Error completing connection: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
  } finally {
    completeConnectionButton.disabled = !preparedConnectionData;
  }
}

async function signCommand() {
  try {
    console.log('Starting signing process...');

    if (!connectedUserId) {
      console.log('Error: You must connect first before signing', 'error');
      return;
    }

    if (!commandHex.value) {
      console.log('Error: You must provide a command to sign', 'error');
      return;
    }

    console.log(`Signing command for user ${connectedUserId}...`);

    const command: Command = {
      url: 'http://localhost:9000/sign',
      body: commandHex.value,
      hex: commandHex.value
    };

    try {
      console.log('Using secure endpoint with JWT token authentication', 'info');
      console.log(localStorage.getItem('virto_jwt_token'));
      const response = await fetch('http://localhost:9000/sign', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${localStorage.getItem('virto_jwt_token')}`
        },
        body: JSON.stringify(command)
      });

      const result = await response.json();

      console.log('Command signed successfully on the server using JWT authentication:', 'success');
      console.log(JSON.stringify(result, null, 2));
    } catch (clientError) {
      console.log(`Error signing the command: ${clientError instanceof Error ? clientError.message : 'Unknown error'}`, 'error');
    }
  } catch (error) {
    console.log(`Error signing the command: ${error instanceof Error ? error.message : 'Unknown error'}`, 'error');
  }
}

prepareRegistrationButton.addEventListener('click', prepareRegistration);
completeRegistrationButton.addEventListener('click', completeRegistration);
prepareConnectionButton.addEventListener('click', prepareConnection);
completeConnectionButton.addEventListener('click', completeConnection);
signButton.addEventListener('click', signCommand);

completeRegistrationButton.disabled = !preparedRegistrationData;
completeConnectionButton.disabled = !preparedConnectionData;

document.addEventListener('DOMContentLoaded', () => {
  loadFromLocalStorage();
});
