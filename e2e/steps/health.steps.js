const { loadFeature, defineFeature } = require("jest-cucumber");
const { api } = require("./helpers");

const feature = loadFeature("./features/health.feature");

defineFeature(feature, (test) => {
  let response;

  test("API health check returns ok", ({ given, when, then, and }) => {
    given("the API server is running", () => {
      // Server should already be running
    });

    when("I request the health endpoint", async () => {
      response = await api.get("/health");
    });

    then(/^the response status should be "(.*)"$/, (status) => {
      expect(response.data.status).toBe(status);
    });

    and(/^the database should be "(.*)"$/, (dbStatus) => {
      expect(response.data.db).toBe(dbStatus);
    });
  });
});
