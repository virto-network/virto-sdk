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

## Usage

### Registration Flow

```ts
import Auth from '@virto-network/sdk';

// Initialize the Auth client
const auth = new Auth('https://your-api-endpoint');

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
  const result = await auth.register(user);
  console.log('Registration successful:', result);
} catch (error) {
  console.error('Registration failed:', error);
}
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

### `Auth` Class

#### Constructor

```ts
constructor(baseUrl: string)
```


#### Methods

##### `register<Profile, Metadata>`

```ts
async register<Profile extends BaseProfile, Metadata extends Record<string, unknown>>(
  user: User<Profile, Metadata>
): Promise<any>
```

Parameters:
- `user`: Object containing user profile and metadata
  - `profile`: Must extend BaseProfile (id, name, displayName)
  - `metadata`: Custom metadata object

Returns: Promise resolving to the registration response
