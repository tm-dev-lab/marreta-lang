// Provider-free, route level (parity with the other stacks): requests hit the Nest app in-process
// via supertest, exercising the ValidationPipe + controller + service logic. Only the Mongoose
// models are mocked, so there is no real MongoDB and no network server.
import 'reflect-metadata';
import { HttpStatus, ValidationPipe } from '@nestjs/common';
import { getModelToken } from '@nestjs/mongoose';
import { Test } from '@nestjs/testing';
import request from 'supertest';
import { BankController } from './bank.controller';
import { BankService } from './bank.service';
import { Account } from './schemas/account.schema';
import { Transaction } from './schemas/transaction.schema';

const OID = '0123456789abcdef01234567';

describe('Bank API (provider-free, route level)', () => {
  let app: any;
  const accountModel = {
    create: jest.fn(),
    findById: jest.fn(),
    updateOne: jest.fn().mockResolvedValue({}),
  };
  const transactionModel = {
    create: jest.fn().mockResolvedValue({ _id: 'txn-1' }),
    find: jest.fn(),
  };

  beforeAll(async () => {
    const moduleRef = await Test.createTestingModule({
      controllers: [BankController],
      providers: [
        BankService,
        { provide: getModelToken(Account.name), useValue: accountModel },
        { provide: getModelToken(Transaction.name), useValue: transactionModel },
      ],
    }).compile();
    app = moduleRef.createNestApplication();
    app.useGlobalPipes(
      new ValidationPipe({ transform: true, errorHttpStatusCode: HttpStatus.UNPROCESSABLE_ENTITY }),
    );
    await app.init();
  });

  afterAll(async () => {
    await app.close();
  });

  it('opens an account (201)', async () => {
    accountModel.create.mockResolvedValue({ _id: OID, owner: 'alice', balance: 0, currency: 'BRL', active: true });
    await request(app.getHttpServer()).post('/accounts').send({ owner: 'alice' }).expect(201);
  });

  it('rejects a withdrawal over the balance (422)', async () => {
    accountModel.findById.mockReturnValue({
      exec: () => Promise.resolve({ _id: OID, balance: 100, currency: 'BRL' }),
    });
    await request(app.getHttpServer()).post(`/accounts/${OID}/withdraw`).send({ amount: 999 }).expect(422);
  });

  it('rejects a missing amount (422)', async () => {
    await request(app.getHttpServer()).post(`/accounts/${OID}/deposit`).send({}).expect(422);
  });
});
