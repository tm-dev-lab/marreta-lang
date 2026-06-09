import os
from datetime import datetime, timezone

from bson import ObjectId
from bson.errors import InvalidId
from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse
from motor.motor_asyncio import AsyncIOMotorClient
from pydantic import BaseModel

MONGO_URI = os.environ.get("MONGO_URI", "mongodb://localhost:27017")
MONGO_DB = os.environ.get("MONGO_DB", "bank")

client = AsyncIOMotorClient(MONGO_URI)
db = client[MONGO_DB]
accounts = db["accounts"]
transactions = db["transactions"]

app = FastAPI()


class CreateAccount(BaseModel):
    owner: str


class Amount(BaseModel):
    amount: int


class Transfer(BaseModel):
    from_account: str
    to_account: str
    amount: int


def account_oid(account_id: str) -> ObjectId:
    try:
        return ObjectId(account_id)
    except (InvalidId, TypeError):
        raise HTTPException(status_code=404, detail="account not found")


def serialize(document: dict) -> dict:
    out = dict(document)
    if "_id" in out and isinstance(out["_id"], ObjectId):
        out["_id"] = str(out["_id"])
    if "created_at" in out and isinstance(out["created_at"], datetime):
        out["created_at"] = out["created_at"].isoformat()
    return out


async def load_account(account_id: str, label: str) -> dict:
    account = await accounts.find_one({"_id": account_oid(account_id)})
    if account is None:
        raise HTTPException(status_code=404, detail=f"{label} not found")
    return account


async def record_transaction(account_id, kind, amount, balance_after, counterparty):
    entry = {
        "account_id": account_id,
        "type": kind,
        "amount": amount,
        "balance_after": balance_after,
        "counterparty": counterparty,
        "created_at": datetime.now(timezone.utc),
    }
    result = await transactions.insert_one(entry)
    entry["_id"] = result.inserted_id
    return serialize(entry)


@app.get("/health")
async def health():
    return {"status": "ok"}


@app.post("/accounts", status_code=201)
async def create_account(body: CreateAccount):
    doc = {"owner": body.owner, "balance": 0, "currency": "BRL", "active": True}
    result = await accounts.insert_one(doc)
    doc["_id"] = result.inserted_id
    return serialize(doc)


@app.get("/accounts/{account_id}")
async def get_account(account_id: str):
    return serialize(await load_account(account_id, "account"))


@app.get("/accounts/{account_id}/balance")
async def get_balance(account_id: str):
    account = await load_account(account_id, "account")
    return {
        "account_id": str(account["_id"]),
        "balance": account["balance"],
        "currency": account["currency"],
    }


@app.post("/accounts/{account_id}/deposit", status_code=201)
async def deposit(account_id: str, body: Amount):
    if body.amount <= 0:
        raise HTTPException(status_code=422, detail="amount must be positive")
    account = await load_account(account_id, "account")
    new_balance = account["balance"] + body.amount
    await accounts.update_one({"_id": account["_id"]}, {"$set": {"balance": new_balance}})
    txn = await record_transaction(account_id, "deposit", body.amount, new_balance, None)
    return {"account_id": account_id, "balance": new_balance, "transaction": txn}


@app.post("/accounts/{account_id}/withdraw", status_code=201)
async def withdraw(account_id: str, body: Amount):
    if body.amount <= 0:
        raise HTTPException(status_code=422, detail="amount must be positive")
    account = await load_account(account_id, "account")
    if account["balance"] < body.amount:
        raise HTTPException(status_code=422, detail="insufficient funds")
    new_balance = account["balance"] - body.amount
    await accounts.update_one({"_id": account["_id"]}, {"$set": {"balance": new_balance}})
    txn = await record_transaction(account_id, "withdraw", body.amount, new_balance, None)
    return {"account_id": account_id, "balance": new_balance, "transaction": txn}


@app.post("/transfers", status_code=201)
async def transfer(body: Transfer):
    if body.amount <= 0:
        raise HTTPException(status_code=422, detail="amount must be positive")
    source = await load_account(body.from_account, "source account")
    target = await load_account(body.to_account, "destination account")
    if source["balance"] < body.amount:
        raise HTTPException(status_code=422, detail="insufficient funds")
    source_balance = source["balance"] - body.amount
    target_balance = target["balance"] + body.amount
    await accounts.update_one({"_id": source["_id"]}, {"$set": {"balance": source_balance}})
    await accounts.update_one({"_id": target["_id"]}, {"$set": {"balance": target_balance}})
    await record_transaction(body.from_account, "transfer_out", body.amount, source_balance, body.to_account)
    await record_transaction(body.to_account, "transfer_in", body.amount, target_balance, body.from_account)
    return {
        "from_account": body.from_account,
        "to_account": body.to_account,
        "amount": body.amount,
        "source_balance": source_balance,
        "target_balance": target_balance,
    }


@app.get("/accounts/{account_id}/transactions")
async def list_transactions(account_id: str):
    cursor = transactions.find({"account_id": account_id}).sort("_id", -1).limit(20)
    rows = [serialize(row) async for row in cursor]
    return {"account_id": account_id, "transactions": rows}
