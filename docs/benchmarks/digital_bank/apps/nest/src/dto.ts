import { IsInt, IsString } from 'class-validator';

export class CreateAccountDto {
  @IsString()
  owner: string;
}

export class AmountDto {
  @IsInt()
  amount: number;
}

export class TransferDto {
  @IsString()
  from_account: string;

  @IsString()
  to_account: string;

  @IsInt()
  amount: number;
}
