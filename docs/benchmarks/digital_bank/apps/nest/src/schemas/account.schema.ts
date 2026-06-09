import { Prop, Schema, SchemaFactory } from '@nestjs/mongoose';
import { HydratedDocument } from 'mongoose';

export type AccountDocument = HydratedDocument<Account>;

@Schema({ collection: 'accounts' })
export class Account {
  @Prop({ required: true })
  owner: string;

  @Prop({ required: true, default: 0 })
  balance: number;

  @Prop({ required: true, default: 'BRL' })
  currency: string;

  @Prop({ required: true, default: true })
  active: boolean;
}

export const AccountSchema = SchemaFactory.createForClass(Account);
