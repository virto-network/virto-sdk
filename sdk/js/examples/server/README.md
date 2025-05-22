# Virto SDK - Server Example

This example shows how to implement a server that uses Virto SDK for authentication and transaction signing, using JWT to ensure the security of user sessions.

## Features

- Express API that acts as an intermediary between the client and the Federate Network
- User and session management
- User registration verification
- WebAuthn authentication completed on the server
- JWT authentication to protect signing operations
- Transaction signing by the authenticated user

## Requirements

- Node.js (v14 or higher)
- npm or yarn

## Installation

1. Clone the repository
2. Install dependencies:

```bash
cd sdk/js/examples/server
npm install
```

## Configuration

The server uses the following environment variables:

- `PORT`: Port where the server will run (default: 9000)
- `JWT_SECRET`: Secret key for signing JWT tokens (default: an example key is used)

For better security in production, set the `JWT_SECRET` variable with a strong and unique value.

## Execution

```bash
npm run dev
```

The server will start at `http://localhost:9000` or on the port specified in the environment variables.

## Endpoints

### User Registration and Connection

- `GET /check-registered/:userId`: Verifies if a user is registered
- `POST /custom-register`: Completes the registration process initiated on the client
- `POST /custom-connect`: Completes the connection process and returns a JWT token for authorization

### Transaction Signing

- `POST /sign`: Signs a transaction using JWT authentication
  - Requires `Authorization: Bearer <token>` header
  - Token verification and userId extraction is performed by the SDK
  - The token has a default validity of 10 minutes
  
## JWT Functionality

1. When a user connects, the serverSDK generates a JWT token that contains:
   - The user ID (userId)
   - The wallet address
   - Issue date (iat)
   - Expiration date (exp)

2. This token is returned to the client, which must store it securely.

3. For signing operations, the client must include the token in the `Authorization` header.

4. The server extracts the token and passes it to the SDK, which:
   - Verifies that the token signature is valid
   - Checks that the token has not expired
   - Confirms that the wallet address in the token matches the current session
   - Extracts the userId and uses it to sign the command

5. If the token is valid, the SDK proceeds with signing; otherwise, it returns a specific error.

## Delegation of Responsibilities

- **Server**: Only responsible for extracting the token from the HTTP request and passing it to the SDK
- **SDK**: Handles all token verification logic, ensuring consistency and security

## Specific Error Responses for JWT

The SDK returns specific error codes that the server transmits to the client:

- `E_JWT_EXPIRED`: The token has expired (401 Unauthorized)
- `E_JWT_INVALID`: The token is invalid or has been tampered with (401 Unauthorized)
- `E_JWT_UNKNOWN`: Unknown error in token verification (401 Unauthorized)
- `E_SESSION_NOT_FOUND`: The session associated with the token does not exist (404 Not Found)
- `E_ADDRESS_MISMATCH`: The token contains an address that does not match the session (401 Unauthorized)

## Client Integration Example

See the [client example](../client/README.md) to understand how a web client integrates with this server. 
