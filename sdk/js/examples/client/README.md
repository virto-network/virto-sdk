# Virto SDK - Client Example

This example shows how to implement a web client that uses Virto SDK to authenticate and authorize the user, and how to handle JWT-based authentication for securely signing transactions.

## Features

- User interface for registration and connection process
- WebAuthn implementation for secure authentication
- JWT authentication support for signing requests
- JWT token storage in localStorage for session persistence
- Automatic token expiration management
- Complete connection and signing flow

## Requirements

- Node.js (v14 or higher)
- npm or yarn
- A server compatible with Virto SDK (see [server example](../server/README.md))
- Modern browser with WebAuthn support

## Installation

1. Clone the repository
2. Install dependencies:

```bash
cd sdk/js/examples/client
npm install
```

## Execution

```bash
npm run dev
```

The application will start in development mode and will be available at `http://localhost:5173` or similar.

## Using the application

1. **Enter a user ID** - Can be any unique identifier
2. **Prepare and complete registration** - If it's the first time using this ID
3. **Prepare and complete connection** - For already registered users
4. **Sign commands** - Once connected, you can sign commands with your wallet

## Session Persistence

This client implements session persistence using localStorage:

1. The JWT token and user ID are automatically saved in localStorage when:
   - A successful connection is completed
   - A valid JWT token is received from the server

2. The session is automatically restored when loading the page:
   - If there is session data in localStorage, it is loaded when starting the application
   - The user can sign commands immediately without having to reconnect

## JWT Authentication Process

### Connection

1. When completing the connection (`completeConnection()`), the client receives a JWT token from the server.
2. This token is stored in memory (`authToken`) and in localStorage for later use.
3. The token contains identity information such as userId and wallet address.

### Signing commands

1. When signing commands (`signCommand()`), the client:
   - Checks if it has a valid JWT token.
   - If it exists, adds the token in the `Authorization: Bearer <token>` header.
   - Sends the request to the secure `/sign` endpoint.

2. If the token has expired:
   - The server returns a 401 error.

## Backend Integration

This client is designed to work with the [example server](../server/README.md) included in this repository, but it can be adapted to work with any backend compatible with the Virto SDK API.

## Execution

```bash
# Start in development mode
npm run dev
```

Once started, you can access the application at [http://localhost:5173](http://localhost:5173).

## Features

### Complete User Registration

This flow uses the SDK's `register()` method, which handles the entire registration process in a single operation within the client.

### Split Client/Server Registration

This flow demonstrates the split architecture:

1. **Step 1: Prepare Registration on Client**
   - Uses the SDK's `prepareRegistration()` method
   - Creates WebAuthn credentials locally in the browser
   - Generates the necessary data to complete the registration

2. **Step 2: Complete Registration on Server**
   - Sends the prepared data to the custom server
   - The server will complete the registration process
   
## How to Test

1. First start the example server located in `../server`
2. Start this client with `npm run dev`
3. Open the browser and use the interface to test both registration flows
