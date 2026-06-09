import { Body, Controller, Get, HttpCode, Param, Post } from '@nestjs/common';
import { BankService } from './bank.service';
import { AmountDto, CreateAccountDto, TransferDto } from './dto';

@Controller()
export class BankController {
  constructor(private readonly bank: BankService) {}

  @Get('health')
  health() {
    return { status: 'ok' };
  }

  @Post('accounts')
  @HttpCode(201)
  createAccount(@Body() body: CreateAccountDto) {
    return this.bank.createAccount(body.owner);
  }

  @Get('accounts/:id')
  getAccount(@Param('id') id: string) {
    return this.bank.getAccount(id);
  }

  @Get('accounts/:id/balance')
  getBalance(@Param('id') id: string) {
    return this.bank.getBalance(id);
  }

  @Post('accounts/:id/deposit')
  @HttpCode(201)
  deposit(@Param('id') id: string, @Body() body: AmountDto) {
    return this.bank.deposit(id, body.amount);
  }

  @Post('accounts/:id/withdraw')
  @HttpCode(201)
  withdraw(@Param('id') id: string, @Body() body: AmountDto) {
    return this.bank.withdraw(id, body.amount);
  }

  @Post('transfers')
  @HttpCode(201)
  transfer(@Body() body: TransferDto) {
    return this.bank.transfer(body.from_account, body.to_account, body.amount);
  }

  @Get('accounts/:id/transactions')
  listTransactions(@Param('id') id: string) {
    return this.bank.listTransactions(id);
  }
}
