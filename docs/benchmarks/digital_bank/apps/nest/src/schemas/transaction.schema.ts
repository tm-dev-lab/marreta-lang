import { Prop, Schema, SchemaFactory } from '@nestjs/mongoose';
import { HydratedDocument } from 'mongoose';

export type TransactionDocument = HydratedDocument<Transaction>;

@Schema({ collection: 'transactions' })
export class Transaction {
  @Prop({ required: true })
  account_id: string;

  @Prop({ required: true })
  type: string;

  @Prop({ required: true })
  amount: number;

  @Prop({ required: true })
  balance_after: number;

  @Prop({ type: String, default: null })
  counterparty: string | null;

  @Prop({ required: true })
  created_at: Date;
}

export const TransactionSchema = SchemaFactory.createForClass(Transaction);
