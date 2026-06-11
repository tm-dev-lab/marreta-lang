package dev.marreta.bench;

import com.fasterxml.jackson.annotation.JsonProperty;
import java.time.Instant;
import org.springframework.data.annotation.Id;
import org.springframework.data.mongodb.core.mapping.Document;

@Document("transactions")
public class Transaction {

    @Id
    @JsonProperty("_id")
    private String id;

    @JsonProperty("account_id")
    private String accountId;

    private String type;
    private long amount;

    @JsonProperty("balance_after")
    private long balanceAfter;

    private String counterparty;

    @JsonProperty("created_at")
    private Instant createdAt;

    public Transaction() {
    }

    public Transaction(String accountId, String type, long amount, long balanceAfter, String counterparty) {
        this.accountId = accountId;
        this.type = type;
        this.amount = amount;
        this.balanceAfter = balanceAfter;
        this.counterparty = counterparty;
        this.createdAt = Instant.now();
    }

    public String getId() {
        return id;
    }

    public void setId(String id) {
        this.id = id;
    }

    public String getAccountId() {
        return accountId;
    }

    public String getType() {
        return type;
    }

    public long getAmount() {
        return amount;
    }

    public long getBalanceAfter() {
        return balanceAfter;
    }

    public String getCounterparty() {
        return counterparty;
    }

    public Instant getCreatedAt() {
        return createdAt;
    }
}
