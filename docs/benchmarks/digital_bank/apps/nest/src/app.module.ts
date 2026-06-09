import { Module } from '@nestjs/common';
import { MongooseModule } from '@nestjs/mongoose';
import { BankController } from './bank.controller';
import { BankService } from './bank.service';
import { Account, AccountSchema } from './schemas/account.schema';
import { Transaction, TransactionSchema } from './schemas/transaction.schema';

@Module({
  imports: [
    MongooseModule.forRoot(process.env.MONGO_URI || 'mongodb://localhost:27017', {
      dbName: process.env.MONGO_DB || 'bank',
    }),
    MongooseModule.forFeature([
      { name: Account.name, schema: AccountSchema },
      { name: Transaction.name, schema: TransactionSchema },
    ]),
  ],
  controllers: [BankController],
  providers: [BankService],
})
export class AppModule {}
