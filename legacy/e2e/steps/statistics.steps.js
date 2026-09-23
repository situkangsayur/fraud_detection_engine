const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makeUser, makeTransaction, makeStandardRule, makePolicy } = require("./helpers");

const feature = loadFeature("./features/statistics.feature");

let seeded = false;

async function seedTestData() {
  if (seeded) return;

  const user = makeUser("stats_user_001");
  await api.post("/user/", user);

  const rule = makeStandardRule(50, "amount", ">", 100000);
  await api.post("/rule/standard", rule);

  const policy = makePolicy("stats_policy_001", [rule]);
  await api.post("/policy/", policy);

  const trx = makeTransaction("stats_trx_001", 200000);
  trx.id_user = "stats_user_001";
  await api.post("/transaction/", trx);

  await api.post("/process/transaction", { id_transaction: "stats_trx_001" });

  seeded = true;
}

defineFeature(feature, (test) => {
  let response;

  test("Get user statistics", ({ given, when, then, and }) => {
    given("the system has seeded data", async () => {
      await seedTestData();
    });

    when("I request user statistics", async () => {
      response = await api.get("/stats/users");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
    });
  });

  test("Get single user statistics", ({ given, when, then, and }) => {
    given("the system has seeded data", async () => {
      await seedTestData();
    });

    when(/^I request statistics for user "(.*)"$/, async (id) => {
      response = await api.get(`/stats/user/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the data should contain "(.*)"$/, (key) => {
      expect(response.data.data).toHaveProperty(key);
    });
  });

  test("Get transaction statistics", ({ given, when, then, and }) => {
    given("the system has seeded data", async () => {
      await seedTestData();
    });

    when("I request transaction statistics", async () => {
      response = await api.get("/stats/transactions");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the data should contain "(.*)"$/, (key) => {
      expect(response.data.data).toHaveProperty(key);
    });
  });

  test("Get policies performance", ({ given, when, then, and }) => {
    given("the system has seeded data", async () => {
      await seedTestData();
    });

    when("I request policies performance", async () => {
      response = await api.get("/stats/policies-performance");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
    });
  });

  test("Get rules performance", ({ given, when, then, and }) => {
    given("the system has seeded data", async () => {
      await seedTestData();
    });

    when("I request rules performance", async () => {
      response = await api.get("/stats/rules-performance");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
    });
  });
});
