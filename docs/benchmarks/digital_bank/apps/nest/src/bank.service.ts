import {
  Injectable,
  NotFoundException,
  UnprocessableEntityException,
} from '@nestjs/common';
import { InjectModel } from '@nestjs/mongoose';
import { isValidObjectId, Model } from 'mongoose';
import { Account } from './schemas/account.schema';
import { Transaction } from './schemas/transaction.schema';

function serialize(doc: any): any {
  const out = doc.toObject ? doc.toObject() : { ...doc };
  if (out._id) out._id = out._id.toString();
  if (out.created_at instanceof Date) out.created_at = out.created_at.toISOString();
  delete out.__v;
  return out;
}

@Injectable()
export class BankService {
  constructor(
    @InjectModel(Account.name) private readonly accounts: Model<Account>,
    @InjectModel(Transaction.name) private readonly transactions: Model<Transaction>,
  ) {}

  private async loadAccount(id: string, label: string) {
    if (!isValidObjectId(id)) throw new NotFoundException(`${label} not found`);
    const account = await this.accounts.findById(id).exec();
    if (!account) throw new NotFoundException(`${label} not found`);
    return account;
  }

  private async recordTransaction(
    accountId: string,
    kind: string,
    amount: number,
    balanceAfter: number,
    counterparty: string | null,
  ) {
    const txn = await this.transactions.create({
      account_id: accountId,
      type: kind,
      amount,
      balance_after: balanceAfter,
      counterparty,
      created_at: new Date(),
    });
    return serialize(txn);
  }

  async createAccount(owner: string) {
    const account = await this.accounts.create({
      owner,
      balance: 0,
      currency: 'BRL',
      active: true,
    });
    return serialize(account);
  }

  async getAccount(id: string) {
    return serialize(await this.loadAccount(id, 'account'));
  }

  async getBalance(id: string) {
    const account = await this.loadAccount(id, 'account');
    return {
      account_id: account._id.toString(),
      balance: account.balance,
      currency: account.currency,
    };
  }

  async deposit(id: string, amount: number) {
    if (amount <= 0) throw new UnprocessableEntityException('amount must be positive');
    const account = await this.loadAccount(id, 'account');
    const newBalance = account.balance + amount;
    await this.accounts.updateOne({ _id: account._id }, { $set: { balance: newBalance } });
    const txn = await this.recordTransaction(id, 'deposit', amount, newBalance, null);
    return { account_id: id, balance: newBalance, transaction: txn };
  }

  async withdraw(id: string, amount: number) {
    if (amount <= 0) throw new UnprocessableEntityException('amount must be positive');
    const account = await this.loadAccount(id, 'account');
    if (account.balance < amount) throw new UnprocessableEntityException('insufficient funds');
    const newBalance = account.balance - amount;
    await this.accounts.updateOne({ _id: account._id }, { $set: { balance: newBalance } });
    const txn = await this.recordTransaction(id, 'withdraw', amount, newBalance, null);
    return { account_id: id, balance: newBalance, transaction: txn };
  }

  async transfer(fromId: string, toId: string, amount: number) {
    if (amount <= 0) throw new UnprocessableEntityException('amount must be positive');
    const source = await this.loadAccount(fromId, 'source account');
    const target = await this.loadAccount(toId, 'destination account');
    if (source.balance < amount) throw new UnprocessableEntityException('insufficient funds');
    const sourceBalance = source.balance - amount;
    const targetBalance = target.balance + amount;
    await this.accounts.updateOne({ _id: source._id }, { $set: { balance: sourceBalance } });
    await this.accounts.updateOne({ _id: target._id }, { $set: { balance: targetBalance } });
    await this.recordTransaction(fromId, 'transfer_out', amount, sourceBalance, toId);
    await this.recordTransaction(toId, 'transfer_in', amount, targetBalance, fromId);
    return {
      from_account: fromId,
      to_account: toId,
      amount,
      source_balance: sourceBalance,
      target_balance: targetBalance,
    };
  }

  async listTransactions(id: string) {
    const rows = await this.transactions
      .find({ account_id: id })
      .sort({ _id: -1 })
      .limit(20)
      .exec();
    return { account_id: id, transactions: rows.map(serialize) };
  }
}
