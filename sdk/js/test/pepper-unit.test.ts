import { PepperManager } from '../src/pepper/pepperManager';
import { CodePepperHandler } from '../src/pepper/codePepperHandler';
import { PepperType, PepperData, PepperConfig } from '../src/types';
import { VError } from '../src/utils/error';

describe('Pepper System - Unit Tests', () => {
  describe('CodePepperHandler', () => {
    let handler: CodePepperHandler;

    beforeEach(() => {
      handler = new CodePepperHandler();
    });

    it('should validate correct 4-digit codes', () => {
      expect(handler.validate('1234')).toBe(true);
      expect(handler.validate('0000')).toBe(true);
      expect(handler.validate('9999')).toBe(true);
    });

    it('should reject codes with wrong length', () => {
      expect(handler.validate('123')).toBe(false);   // too short
      expect(handler.validate('12345')).toBe(false); // too long
      expect(handler.validate('')).toBe(false);      // empty
    });

    it('should reject codes with non-numeric characters', () => {
      expect(handler.validate('12ab')).toBe(false);
      expect(handler.validate('a234')).toBe(false);
      expect(handler.validate('12.3')).toBe(false);
      expect(handler.validate('12 3')).toBe(false); // space
    });

    it('should handle different lengths when configured', () => {
      const handler6 = new CodePepperHandler(6);
      expect(handler6.validate('123456')).toBe(true);
      expect(handler6.validate('1234')).toBe(false); // wrong length for 6-digit handler
    });

    it('should prepare values by trimming whitespace', () => {
      expect(handler.prepare(' 1234 ')).toBe('1234');
      expect(handler.prepare('1234')).toBe('1234');
    });

    it('should have correct type', () => {
      expect(handler.type).toBe(PepperType.CODE);
    });
  });

  describe('PepperManager', () => {
    describe('Without Configuration', () => {
      let manager: PepperManager;

      beforeEach(() => {
        manager = new PepperManager();
      });

      it('should not require pepper by default', () => {
        expect(manager.isPepperRequired()).toBe(false);
      });

      it('should return null for configured pepper type', () => {
        expect(manager.getConfiguredPepperType()).toBe(null);
      });

      it('should support CODE pepper type by default', () => {
        const supportedTypes = manager.getSupportedPepperTypes();
        expect(supportedTypes).toContain(PepperType.CODE);
      });

      it('should have CODE handler registered by default', () => {
        const handler = manager.getHandler(PepperType.CODE);
        expect(handler).toBeDefined();
        expect(handler?.type).toBe(PepperType.CODE);
      });
    });

    describe('With CODE Configuration', () => {
      let manager: PepperManager;
      const config: PepperConfig = {
        type: PepperType.CODE
      };

      beforeEach(() => {
        manager = new PepperManager(config);
      });

      it('should require pepper when configured', () => {
        expect(manager.isPepperRequired()).toBe(true);
      });

      it('should return configured pepper type', () => {
        expect(manager.getConfiguredPepperType()).toBe(PepperType.CODE);
      });

      it('should validate correct pepper data', () => {
        const pepperData: PepperData = {
          type: PepperType.CODE,
          value: '1234'
        };
        expect(manager.validatePepper(pepperData)).toBe(true);
      });

      it('should reject invalid pepper data', () => {
        const pepperData: PepperData = {
          type: PepperType.CODE,
          value: 'abc'
        };
        expect(manager.validatePepper(pepperData)).toBe(false);
      });

      it('should prepare and validate pepper data', () => {
        const pepperData: PepperData = {
          type: PepperType.CODE,
          value: ' 1234 '
        };
        const prepared = manager.preparePepper(pepperData);
        expect(prepared.value).toBe('1234');
        expect(prepared.type).toBe(PepperType.CODE);
      });

      it('should throw error for invalid pepper value during preparation', () => {
        const pepperData: PepperData = {
          type: PepperType.CODE,
          value: 'invalid'
        };
        
        try {
          manager.preparePepper(pepperData);
          fail('Expected to throw VError');
        } catch (error) {
          expect(error).toBeInstanceOf(VError);
          expect((error as VError).code).toBe('E_INVALID_PEPPER_VALUE');
        }
      });
    });

    describe('No Pepper Configuration', () => {
      let manager: PepperManager;

      beforeEach(() => {
        manager = new PepperManager(); // No config = no pepper required
      });

      it('should not require pepper when not configured', () => {
        expect(manager.isPepperRequired()).toBe(false);
      });

      it('should return null for configured type', () => {
        expect(manager.getConfiguredPepperType()).toBe(null);
      });
    });

    describe('Error Handling', () => {
      let manager: PepperManager;

      beforeEach(() => {
        manager = new PepperManager();
      });

      it('should throw error for unsupported pepper type in validation', () => {
        const pepperData = {
          type: 'UNSUPPORTED_TYPE' as PepperType,
          value: '1234'
        };
        
        try {
          manager.validatePepper(pepperData);
          fail('Expected to throw VError');
        } catch (error) {
          expect(error).toBeInstanceOf(VError);
          expect((error as VError).code).toBe('E_INVALID_PEPPER_TYPE');
        }
      });

      it('should throw error for unsupported pepper type in preparation', () => {
        const pepperData = {
          type: 'UNSUPPORTED_TYPE' as PepperType,
          value: '1234'
        };
        
        try {
          manager.preparePepper(pepperData);
          fail('Expected to throw VError');
        } catch (error) {
          expect(error).toBeInstanceOf(VError);
          expect((error as VError).code).toBe('E_INVALID_PEPPER_TYPE');
        }
      });
    });

    describe('Handler Registration', () => {
      let manager: PepperManager;

      beforeEach(() => {
        manager = new PepperManager();
      });

      it('should allow registering custom handlers', () => {
        const customHandler = {
          type: 'CUSTOM_TYPE' as PepperType,
          validate: (value: string) => value === 'test',
          prepare: (value: string) => value.toLowerCase()
        };

        manager.registerHandler(customHandler);
        const retrievedHandler = manager.getHandler('CUSTOM_TYPE' as PepperType);
        expect(retrievedHandler).toBe(customHandler);
      });

      it('should return undefined for non-existent handlers', () => {
        const handler = manager.getHandler('NON_EXISTENT' as PepperType);
        expect(handler).toBeUndefined();
      });

      it('should include custom handlers in supported types', () => {
        const customHandler = {
          type: 'CUSTOM_TYPE' as PepperType,
          validate: () => true,
        };

        manager.registerHandler(customHandler);
        const supportedTypes = manager.getSupportedPepperTypes();
        expect(supportedTypes).toContain('CUSTOM_TYPE' as PepperType);
      });
    });
  });
}); 