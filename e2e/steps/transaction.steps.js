const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makeTransaction } = require("./helpers");

const feature = loadFeature("./features/transaction.feature");

defineFeature(feature, (test) => {
  let response;

  test("Create a new transaction", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I create a transaction with id "(.*)" and amount (\d+)$/, async (id, amount) => {
      response = await api.post("/transaction/", makeTransaction(id, Number(amount)));
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get transaction by ID", ({ given, when, then, and }) => {
    given(/^a transaction with id "(.*)" exists$/, async (id) => {
      await api.post("/transaction/", makeTransaction(id));
    });

    when(/^I get the transaction with id "(.*)"$/, async (id) => {
      response = await api.get(`/transaction/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the transaction amount should be (\d+)$/, (amount) => {
      expect(response.data.data.amount).toBe(Number(amount));
    });
  });

  test("List all transactions", ({ given, when, then, and }) => {
    given(/^a transaction with id "(.*)" exists$/, async (id) => {
      await api.post("/transaction/", makeTransaction(id));
    });

    when("I list all transactions", async () => {
      response = await api.get("/transaction/");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a non-empty list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
      expect(response.data.data.length).toBeGreaterThan(0);
    });
  });

  test("Delete a transaction", ({ given, when, then, and }) => {
    given(/^a transaction with id "(.*)" exists$/, async (id) => {
      await api.post("/transaction/", makeTransaction(id));
    });

    when(/^I delete the transaction with id "(.*)"$/, async (id) => {
      response = await api.delete(`/transaction/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get non-existent transaction", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I get the transaction with id "(.*)"$/, async (id) => {
      response = await api.get(`/transaction/${id}`);
    });

    then("the response should not be successful", () => {
      expect(response.data.success).toBe(false);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });
});
