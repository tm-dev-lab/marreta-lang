package dev.marreta.bench;

import dev.marreta.bench.Dtos.Amount;
import dev.marreta.bench.Dtos.CreateAccount;
import dev.marreta.bench.Dtos.Transfer;
import jakarta.validation.Valid;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.springframework.http.HttpStatus;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class BankController {

    private final AccountRepository accounts;
    private final TransactionRepository transactions;

    public BankController(AccountRepository accounts, TransactionRepository transactions) {
        this.accounts = accounts;
        this.transactions = transactions;
    }

    @GetMapping("/health")
    public Map<String, Object> health() {
        return Map.of("status", "ok");
    }

    @PostMapping("/accounts")
    @ResponseStatus(HttpStatus.CREATED)
    public Account createAccount(@Valid @RequestBody CreateAccount body) {
        return accounts.save(new Account(body.owner(), 0, "BRL", true));
    }

    @GetMapping("/accounts/{id}")
    public Account getAccount(@PathVariable String id) {
        return loadAccount(id, "account");
    }

    @GetMapping("/accounts/{id}/balance")
    public Map<String, Object> getBalance(@PathVariable String id) {
        Account account = loadAccount(id, "account");
        return ordered("account_id", account.getId(), "balance", account.getBalance(), "currency", account.getCurrency());
    }

    @PostMapping("/accounts/{id}/deposit")
    @ResponseStatus(HttpStatus.CREATED)
    public Map<String, Object> deposit(@PathVariable String id, @Valid @RequestBody Amount body) {
        requirePositive(body.amount());
        Account account = loadAccount(id, "account");
        long newBalance = account.getBalance() + body.amount();
        account.setBalance(newBalance);
        accounts.save(account);
        Transaction txn = record(id, "deposit", body.amount(), newBalance, null);
        return ordered("account_id", id, "balance", newBalance, "transaction", txn);
    }

    @PostMapping("/accounts/{id}/withdraw")
    @ResponseStatus(HttpStatus.CREATED)
    public Map<String, Object> withdraw(@PathVariable String id, @Valid @RequestBody Amount body) {
        requirePositive(body.amount());
        Account account = loadAccount(id, "account");
        if (account.getBalance() < body.amount()) {
            throw new ApiException(HttpStatus.UNPROCESSABLE_ENTITY, "insufficient funds");
        }
        long newBalance = account.getBalance() - body.amount();
        account.setBalance(newBalance);
        accounts.save(account);
        Transaction txn = record(id, "withdraw", body.amount(), newBalance, null);
        return ordered("account_id", id, "balance", newBalance, "transaction", txn);
    }

    @PostMapping("/transfers")
    @ResponseStatus(HttpStatus.CREATED)
    public Map<String, Object> transfer(@Valid @RequestBody Transfer body) {
        requirePositive(body.amount());
        Account source = loadAccount(body.fromAccount(), "source account");
        Account target = loadAccount(body.toAccount(), "destination account");
        if (source.getBalance() < body.amount()) {
            throw new ApiException(HttpStatus.UNPROCESSABLE_ENTITY, "insufficient funds");
        }
        long sourceBalance = source.getBalance() - body.amount();
        long targetBalance = target.getBalance() + body.amount();
        source.setBalance(sourceBalance);
        target.setBalance(targetBalance);
        accounts.save(source);
        accounts.save(target);
        record(body.fromAccount(), "transfer_out", body.amount(), sourceBalance, body.toAccount());
        record(body.toAccount(), "transfer_in", body.amount(), targetBalance, body.fromAccount());
        return ordered(
                "from_account", body.fromAccount(),
                "to_account", body.toAccount(),
                "amount", body.amount(),
                "source_balance", sourceBalance,
                "target_balance", targetBalance);
    }

    @GetMapping("/accounts/{id}/transactions")
    public Map<String, Object> listTransactions(@PathVariable String id) {
        List<Transaction> rows = transactions.findTop20ByAccountIdOrderByIdDesc(id);
        return ordered("account_id", id, "transactions", rows);
    }

    private Account loadAccount(String id, String label) {
        try {
            return accounts.findById(id)
                    .orElseThrow(() -> new ApiException(HttpStatus.NOT_FOUND, label + " not found"));
        } catch (ApiException e) {
            throw e;
        } catch (RuntimeException invalidId) {
            // An id that is not a valid ObjectId fails conversion: treat it as not found, like the
            // other contenders, rather than a 500.
            throw new ApiException(HttpStatus.NOT_FOUND, label + " not found");
        }
    }

    private Transaction record(String accountId, String type, long amount, long balanceAfter, String counterparty) {
        return transactions.save(new Transaction(accountId, type, amount, balanceAfter, counterparty));
    }

    private static void requirePositive(long amount) {
        if (amount <= 0) {
            throw new ApiException(HttpStatus.UNPROCESSABLE_ENTITY, "amount must be positive");
        }
    }

    private static Map<String, Object> ordered(Object... kv) {
        LinkedHashMap<String, Object> out = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            out.put((String) kv[i], kv[i + 1]);
        }
        return out;
    }
}
