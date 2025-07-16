import express, { Request, Response } from 'express';
import cors from 'cors';
import { ServerSDK, PreparedRegistrationData, PreparedConnectionData, WalletType, Command, IStorage } from '../../../src/index';
import { MongoDBStorage } from './storage/MongoDBStorage';
import { Session } from '../../../src/manager';

const app = express();
const port = process.env.PORT || 9000;

app.use(cors());
app.use(express.json());

// MongoDB configuration
const mongoUrl = process.env.MONGODB_URL || 'mongodb://localhost:27017';
const dbName = process.env.MONGODB_DB_NAME || 'virto_sessions';

async function createStorage(): Promise<IStorage<Session>> {
  console.log('Initializing MongoDB storage...');
  const mongoDbStorage = new MongoDBStorage({
    url: mongoUrl,
    dbName: dbName,
    collectionName: 'user_sessions'
  });

  try {
    await mongoDbStorage.connect();

    // Test MongoDB connection
    await mongoDbStorage.store('test', { message: 'MongoDB connection test' });
    await mongoDbStorage.get('test');
    await mongoDbStorage.remove('test');

    return mongoDbStorage as IStorage<Session>;
  } catch (error) {
    console.error('Failed to connect to MongoDB:', error);
    throw error;
  }
}

(async () => {
  try {
    // Initialize MongoDB and ServerSDK
    const mongoStorage = await createStorage();

    //@ts-ignore
    const serverSdk = new ServerSDK({
      federate_server: 'http://localhost:3000/api',
      provider_url: 'ws://localhost:12281',
      config: {
        wallet: WalletType.POLKADOT,
        jwt: {
          secret: process.env.JWT_SECRET || 'virto-server-example-secret-key-change-in-production',
          expiresIn: '10m'
        }
      }
    },
      () => { throw new Error("SubeFn not needed in server environment"); }, // subeFn
      () => { throw new Error("JsWalletFn not needed in server environment"); }, // jsWalletFn
      mongoStorage
    );

    app.get('/check-registered/:userId', async (req: Request, res: Response) => {
      try {
        const userId = req.params.userId;
        const isRegistered = await serverSdk.auth.isRegistered(userId);

        res.json({
          userId,
          isRegistered
        });
      } catch (error: any) {
        console.error('Error checking registration:', error);
        res.status(500).json({
          error: 'Error checking if the user is registered',
          details: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    });

    app.post('/custom-register', async (req: Request, res: Response) => {
      try {
        const preparedData: PreparedRegistrationData = req.body;

        if (!preparedData.userId || !preparedData.attestationResponse || !preparedData.blockNumber) {
          return res.status(400).json({ error: 'Incomplete registration data' });
        }

        console.log('Data received from client:', JSON.stringify(preparedData, null, 2));

        const result = await serverSdk.auth.completeRegistration(preparedData);

        res.json({
          success: true,
          message: 'Registration completed successfully',
          data: result
        });
      } catch (error: any) {
        console.error('Error in registration process:', error);
        res.status(500).json({
          success: false,
          error: 'Error completing registration',
          details: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    });

    app.post('/custom-connect', async (req: Request, res: Response) => {
      try {
        const preparedData: PreparedConnectionData = req.body;

        if (!preparedData.userId || !preparedData.assertionResponse || !preparedData.blockNumber) {
          return res.status(400).json({ error: 'Incomplete connection data' });
        }

        console.log('Connection data received from client:', JSON.stringify(preparedData, null, 2));

        const result = await serverSdk.auth.completeConnection(preparedData);

        res.json({
          success: true,
          message: 'Connection completed successfully',
          sessionId: result.session.userId,
          address: result.session.address,
          createdAt: result.session.createdAt,
          token: result.token
        });
      } catch (error: any) {
        console.error('Error in connection process:', error);
        res.status(500).json({
          success: false,
          error: 'Error completing connection',
          details: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    });

    app.post('/sign', async (req: Request, res: Response) => {
      try {
        const authHeader = req.headers.authorization;

        if (!authHeader || !authHeader.startsWith('Bearer ')) {
          return res.status(401).json({
            success: false,
            error: 'No token provided or invalid format'
          });
        }

        const token = authHeader.split(' ')[1];
        const commandData: Command = req.body;

        if (!commandData || !commandData.hex) {
          return res.status(400).json({ error: 'Incomplete data for signing the command' });
        }

        const signResult = await serverSdk.auth.signWithToken(token, commandData);

        res.json({
          success: true,
          message: 'Command signed successfully',
          ...signResult
        });
      } catch (error: any) {
        console.error('Error signing the command:', error);

        if (error.code === 'E_JWT_EXPIRED') {
          return res.status(401).json({
            success: false,
            error: 'Token has expired, please reconnect',
            code: error.code
          });
        } else if (error.code === 'E_JWT_INVALID') {
          return res.status(401).json({
            success: false,
            error: 'Invalid token',
            code: error.code
          });
        } else if (error.code === 'E_SESSION_NOT_FOUND') {
          return res.status(404).json({
            success: false,
            error: 'Session not found, please reconnect',
            code: error.code
          });
        } else if (error.code === 'E_ADDRESS_MISMATCH') {
          return res.status(401).json({
            success: false,
            error: 'Token address does not match session address',
            code: error.code
          });
        }

        res.status(500).json({
          success: false,
          error: 'Error signing the command',
          details: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    });

    app.listen(port, () => {
      console.log(`⚡️ Server running at http://localhost:${port}`);

      console.log(`MongoDB URL: ${mongoUrl}`);

      console.log('Available endpoints:');
      console.log(`- GET  /check-registered/:userId`);
      console.log(`- POST /custom-register`);
      console.log(`- POST /custom-connect`);
      console.log(`- POST /sign (Secured with JWT)`);
    });
  } catch (error) {
    console.error('Failed to initialize server:', error);
    process.exit(1);
  }
})(); 