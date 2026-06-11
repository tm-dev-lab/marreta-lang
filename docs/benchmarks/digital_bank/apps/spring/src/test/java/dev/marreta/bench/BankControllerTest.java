package dev.marreta.bench;

import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.when;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.mock.mockito.MockBean;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;

// Provider-free, route level (parity with the other stacks): requests go through the MVC slice
// (validation + controller + advice), with the repositories mocked, so no real MongoDB and no
// running server.
@WebMvcTest(BankController.class)
class BankControllerTest {

    @Autowired
    MockMvc mvc;

    @MockBean
    AccountRepository accounts;

    @MockBean
    TransactionRepository transactions;

    @Test
    void opensAccount() throws Exception {
        Account a = new Account("alice", 0, "BRL", true);
        a.setId("acc-1");
        when(accounts.save(any())).thenReturn(a);
        mvc.perform(post("/accounts").contentType(MediaType.APPLICATION_JSON).content("{\"owner\":\"alice\"}"))
                .andExpect(status().isCreated());
    }

    @Test
    void rejectsWithdrawalOverBalance() throws Exception {
        Account a = new Account("alice", 100, "BRL", true);
        a.setId("acc-1");
        when(accounts.findById("acc-1")).thenReturn(Optional.of(a));
        mvc.perform(post("/accounts/acc-1/withdraw").contentType(MediaType.APPLICATION_JSON).content("{\"amount\":999}"))
                .andExpect(status().isUnprocessableEntity());
    }

    @Test
    void rejectsMissingAmount() throws Exception {
        mvc.perform(post("/accounts/acc-1/deposit").contentType(MediaType.APPLICATION_JSON).content("{}"))
                .andExpect(status().isUnprocessableEntity());
    }
}
