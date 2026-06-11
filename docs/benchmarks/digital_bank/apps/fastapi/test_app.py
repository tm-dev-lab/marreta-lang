"""Provider-free tests: the motor collections are replaced with async mocks, so these run the
real route logic in-process (Starlette TestClient) with no MongoDB and no network server."""
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from bson import ObjectId
from fastapi.testclient import TestClient

import app as appmod

client = TestClient(appmod.app)
OID = "0123456789abcdef01234567"


@pytest.fixture(autouse=True)
def mock_collections(monkeypatch):
    accounts = AsyncMock()
    transactions = AsyncMock()
    monkeypatch.setattr(appmod, "accounts", accounts)
    monkeypatch.setattr(appmod, "transactions", transactions)
    return accounts, transactions


def test_opens_account(mock_collections):
    accounts, _ = mock_collections
    accounts.insert_one.return_value = SimpleNamespace(inserted_id=ObjectId(OID))
    r = client.post("/accounts", json={"owner": "alice"})
    assert r.status_code == 201
    assert r.json()["owner"] == "alice"


def test_rejects_overdraw(mock_collections):
    accounts, _ = mock_collections
    accounts.find_one.return_value = {"_id": ObjectId(OID), "balance": 100, "currency": "BRL"}
    r = client.post(f"/accounts/{OID}/withdraw", json={"amount": 999})
    assert r.status_code == 422


def test_rejects_missing_amount():
    r = client.post(f"/accounts/{OID}/deposit", json={})
    assert r.status_code == 422
