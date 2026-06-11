package dev.marreta.bench;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotNull;

/** Request bodies, validated by Bean Validation (a missing field is a 422, like the others). */
public final class Dtos {
    private Dtos() {
    }

    public record CreateAccount(@NotNull String owner) {
    }

    public record Amount(@NotNull Long amount) {
    }

    public record Transfer(
            @JsonProperty("from_account") @NotNull String fromAccount,
            @JsonProperty("to_account") @NotNull String toAccount,
            @NotNull Long amount) {
    }
}
