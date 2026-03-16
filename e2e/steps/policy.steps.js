const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makePolicy, makeStandardRule } = require("./helpers");

const feature = loadFeature("./features/policy.feature");

defineFeature(feature, (test) => {
  let response;

  test("Create a new policy", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I create a policy with id "(.*)"$/, async (id) => {
      response = await api.post("/policy/", makePolicy(id, [makeStandardRule()]));
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get policy by ID", ({ given, when, then, and }) => {
    given(/^a policy with id "(.*)" exists$/, async (id) => {
      await api.post("/policy/", makePolicy(id, [makeStandardRule()]));
    });

    when(/^I get the policy with id "(.*)"$/, async (id) => {
      response = await api.get(`/policy/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the policy name should be "(.*)"$/, (name) => {
      expect(response.data.data.name).toBe(name);
    });
  });

  test("List all policies", ({ given, when, then, and }) => {
    given(/^a policy with id "(.*)" exists$/, async (id) => {
      await api.post("/policy/", makePolicy(id));
    });

    when("I list all policies", async () => {
      response = await api.get("/policy/");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a non-empty list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
      expect(response.data.data.length).toBeGreaterThan(0);
    });
  });

  test("Delete a policy", ({ given, when, then, and }) => {
    given(/^a policy with id "(.*)" exists$/, async (id) => {
      await api.post("/policy/", makePolicy(id));
    });

    when(/^I delete the policy with id "(.*)"$/, async (id) => {
      response = await api.delete(`/policy/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get non-existent policy", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I get the policy with id "(.*)"$/, async (id) => {
      response = await api.get(`/policy/${id}`);
    });

    then("the response should not be successful", () => {
      expect(response.data.success).toBe(false);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });
});
