# Virto SDK Demo

This demo showcases the Virto SDK with different storage options for session management.

## Features

- **Multiple Storage Options**: Choose between localStorage, IndexedDB, and sessionStorage
- **Session Management**: Register users, connect, and sign transactions

## Storage Options

### localStorage
- **Persistence**: Data persists until manually cleared

### IndexedDB
- **Persistence**: Data persists until manually cleared

### sessionStorage
- **Persistence**: Data cleared when tab closes

## Getting Started

1. **Install dependencies**:
   ```bash
   npm install
   ```

2. **Start the development environment**:
   ```bash
   npm run dev
   ```

3. **Open your browser** and navigate to the provided URL (usually `http://localhost:3017`)

1. **Register User**: Fill in user details and click "Register User"
2. **Connect**: Click "Connect" to establish a connection
3. **Sign Transaction**: Enter a command and click "Sign" to test transaction signing

## Troubleshooting

### Storage Issues
- **IndexedDB not working**: Check if your browser supports IndexedDB
- **Data not persisting**: Verify storage type selection
- **Quota exceeded**: Clear storage or use IndexedDB for larger capacity

### SDK Issues
- **Registration fails**: Check console for detailed error messages
- **Connection fails**: Ensure the provider URL is accessible
