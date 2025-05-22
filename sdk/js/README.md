# @virto-network/sdk

A TypeScript SDK for implementing WebAuthn authentication in web applications.

## Installation

```bash
npm install @virto-network/sdk
```

## Features

- WebAuthn registration flow
- Sign extrinsics
- TypeScript support with generic types for user profiles and metadata
- Split client/server architecture support

## Usage

### Standard Registration Flow

```ts
import { SDK } from '@virto-network/sdk';

// Initialize the SDK
const sdk = new SDK({
  federate_server: 'https://your-api-endpoint',
  provider_url: 'https://your-provider-url',
  config: {
    wallet: WalletType.POLKADOT
  }
}, subeFn, jsWalletBuilder);

// Define your user object
const user = {
  profile: {
    id: "user123",
    name: "john.doe", 
    displayName: "John Doe"
  },
  metadata: {
    role: "user",
    // Add any custom metadata
  }
};

// Register the user
try {
  const result = await sdk.auth.register(user);
  console.log('Registration successful:', result);
} catch (error) {
  console.error('Registration failed:', error);
}
```

### Split Client/Server Architecture

For applications that need to split the registration process between client and server:

#### Client-Side (Browser)

```ts
import { SDK } from '@virto-network/sdk';

// Initialize the client SDK
const sdk = new SDK({
  federate_server: 'https://your-api-endpoint',
  provider_url: 'https://your-provider-url',
  config: {
    wallet: WalletType.POLKADOT
  }
}, subeFn, jsWalletBuilder);

// Prepare registration on client-side
async function registerUser(user) {
  try {
    // This step uses WebAuthn (navigator.credentials) which only works in browser
    const preparedData = await sdk.auth.prepareRegistration(user);
    
    // Send prepared data to your custom server endpoint
    const response = await fetch('https://your-server/custom-register', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(preparedData)
    });
    
    return await response.json();
  } catch (error) {
    console.error('Client-side registration preparation failed:', error);
  }
}
```

#### Server-Side (Node.js)

```ts
import { ServerSDK } from '@virto-network/sdk';

// Initialize the server SDK
const serverSdk = new ServerSDK({
  federate_server: 'https://your-api-endpoint',
  provider_url: 'https://your-provider-url',
  config: {
    wallet: WalletType.POLKADOT,
    jwt: {
      secret: process.env.JWT_SECRET || 'your-secret',
      expiresIn: '10m'
    }
  }
});

// Express endpoint example
app.post('/custom-register', async (req, res) => {
  try {
    // Complete registration process on server-side
    const preparedData = req.body;
    const result = await serverSdk.auth.completeRegistration(preparedData);
    
    // Return result to client
    res.json(result);
  } catch (error) {
    console.error('Server-side registration completion failed:', error);
    res.status(500).json({ error: error.message });
  }
});
```

## Development

### Prerequisites

- Node.js (Latest LTS version recommended)
- npm or yarn

### Setup

1. Clone the repository
2. Install dependencies:


```bash
npm install
```
### Available Scripts

- `npm run dev` - Starts the development server using Vite
- `npm run build` - Builds the SDK for production
- `npm run test:e2e` - Runs end-to-end tests with Puppeteer

### Running Tests

The project uses Jest and Puppeteer for testing. The E2E tests simulate a complete WebAuthn registration flow using a virtual authenticator.

To run all tests:

```bash
npm run test:e2e
```

## API Reference

### Client SDK Classes

#### `SDK` Class

Main class for browser environments.

#### `Auth` Class

Class for authentication operations in browser environments.

##### Methods

###### `register<Profile, Metadata>`

Complete registration flow in a single call.

```ts
async register<Profile extends BaseProfile>(
  user: User<Profile>
): Promise<any>
```

###### `prepareRegistration<Profile, Metadata>`

Prepares registration data on the client side using WebAuthn.

```ts
async prepareRegistration<Profile extends BaseProfile>(
  user: User<Profile>
): Promise<PreparedRegistrationData>
```

### Server SDK Classes

#### `ServerSDK` Class

Main class for server (Node.js) environments.

#### `ServerAuth` Class

Class for authentication operations in server environments.

##### Methods

###### `completeRegistration`

Completes the registration process on the server side.

```ts
async completeRegistration(
  preparedData: PreparedRegistrationData
): Promise<any>
```

###### `isRegistered`

Check if a user is registered.

```ts
async isRegistered(userId: string): Promise<boolean>
```

### Shared Types

#### `PreparedRegistrationData`

Data structure prepared by the client to be sent to the server.

```ts
interface PreparedRegistrationData {
  userId: string;
  attestationResponse: PreparedCredentialData;
  blockNumber: number;
}
```
