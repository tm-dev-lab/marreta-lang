import 'reflect-metadata';
import { HttpStatus, ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module';

async function bootstrap() {
  const app = await NestFactory.create(AppModule, { logger: ['error', 'warn'] });
  // A failed body validation is a 422, matching the other contenders (Nest defaults to 400).
  app.useGlobalPipes(
    new ValidationPipe({ transform: true, errorHttpStatusCode: HttpStatus.UNPROCESSABLE_ENTITY }),
  );
  await app.listen(8080, '0.0.0.0');
}

bootstrap();
