const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makeStandardRule, makeVelocityRule } = require("./helpers");

const feature = loadFeature("./features/rule.feature");

defineFeature(feature, (test) => {
  let response;
  let ruleId;

  test("Create a standard rule", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(
      /^I create a standard rule with field "(.*)" operator "(.*)" and value (\d+)$/,
      async (field, operator, value) => {
        response = await api.post("/rule/standard", makeStandardRule(50, field, operator, Number(value)));
      }
    );

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Create a velocity rule", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(
      /^I create a velocity rule with field "(.*)" time_range "(.*)" and threshold (\d+)$/,
      async (field, timeRange, threshold) => {
        response = await api.post(
          "/rule/velocity",
          makeVelocityRule(40, field, timeRange, Number(threshold))
        );
      }
    );

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("List all rules", ({ given, when, then, and }) => {
    given("a standard rule exists", async () => {
      const res = await api.post("/rule/standard", makeStandardRule());
      ruleId = res.data.data._id;
    });

    when("I list all rules", async () => {
      response = await api.get("/rule/");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a non-empty list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
      expect(response.data.data.length).toBeGreaterThan(0);
    });
  });

  test("Get a rule by ID", ({ given, when, then, and }) => {
    given("a standard rule exists", async () => {
      const res = await api.post("/rule/standard", makeStandardRule());
      ruleId = res.data.data._id;
    });

    when("I get the rule by its ID", async () => {
      response = await api.get(`/rule/${ruleId}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the rule description should be present", () => {
      expect(response.data.data.description).toBeTruthy();
    });
  });

  test("Delete a rule", ({ given, when, then, and }) => {
    given("a standard rule exists", async () => {
      const res = await api.post("/rule/standard", makeStandardRule());
      ruleId = res.data.data._id;
    });

    when("I delete the rule by its ID", async () => {
      response = await api.delete(`/rule/${ruleId}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });
});
