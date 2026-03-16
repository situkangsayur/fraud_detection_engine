const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makeUser, makeTransaction, makeStandardRule, makeVelocityRule } = require("./helpers");

const feature = loadFeature("./features/process.feature");

defineFeature(feature, (test) => {
  let response;

  test("Process a high-amount transaction as fraud", ({ given, and, when, then }) => {
    given(/^a user "(.*)" exists in the system$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    and(/^a standard rule with risk_point (\d+) for amount > (\d+) exists$/, async (riskPoint, value) => {
      await api.post("/rule/standard", makeStandardRule(Number(riskPoint), "amount", ">", Number(value)));
    });

    and(/^a transaction "(.*)" with amount (\d+) exists$/, async (id, amount) => {
      const trx = makeTransaction(id, Number(amount));
      trx.id_user = "proc_user_001";
      await api.post("/transaction/", trx);
    });

    when(/^I process the transaction "(.*)"$/, async (id) => {
      response = await api.post("/process/transaction", { id_transaction: id });
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the risk score should be at least (\d+)$/, (minScore) => {
      expect(response.data.data.risk_score).toBeGreaterThanOrEqual(Number(minScore));
    });

    and(/^the detected status should be "(.*)"$/, (status) => {
      expect(response.data.data.detected_status).toBe(status);
    });

    and("matched rules should not be empty", () => {
      expect(response.data.data.matched_rules.length).toBeGreaterThan(0);
    });
  });

  test("Process a transaction and get evaluated result", ({ given, and, when, then }) => {
    given(/^a user "(.*)" exists in the system$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    and(/^a standard rule with risk_point (\d+) for amount > (\d+) exists$/, async (riskPoint, value) => {
      await api.post("/rule/standard", makeStandardRule(Number(riskPoint), "amount", ">", Number(value)));
    });

    and(/^a transaction "(.*)" with amount (\d+) exists$/, async (id, amount) => {
      const trx = makeTransaction(id, Number(amount));
      trx.id_user = "proc_user_002";
      await api.post("/transaction/", trx);
    });

    when(/^I process the transaction "(.*)"$/, async (id) => {
      response = await api.post("/process/transaction", { id_transaction: id });
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the detected status should be present", () => {
      expect(["normal", "suspect", "fraud"]).toContain(response.data.data.detected_status);
    });
  });

  test("Process non-existent transaction", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I process the transaction "(.*)"$/, async (id) => {
      response = await api.post("/process/transaction", { id_transaction: id });
    });

    then("the response should not be successful", () => {
      expect(response.data.success).toBe(false);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Process transaction with velocity rule", ({ given, and, when, then }) => {
    given(/^a user "(.*)" exists in the system$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    and(/^a velocity rule with risk_point (\d+) exists$/, async (riskPoint) => {
      await api.post("/rule/velocity", makeVelocityRule(Number(riskPoint)));
    });

    and(/^a transaction "(.*)" with amount (\d+) exists$/, async (id, amount) => {
      const trx = makeTransaction(id, Number(amount));
      trx.id_user = "proc_user_003";
      await api.post("/transaction/", trx);
    });

    when(/^I process the transaction "(.*)"$/, async (id) => {
      response = await api.post("/process/transaction", { id_transaction: id });
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the risk score should be at least (\d+)$/, (minScore) => {
      expect(response.data.data.risk_score).toBeGreaterThanOrEqual(Number(minScore));
    });

    and("matched rules should not be empty", () => {
      expect(response.data.data.matched_rules.length).toBeGreaterThan(0);
    });
  });
});
