const { loadFeature, defineFeature } = require("jest-cucumber");
const { api, makeUser } = require("./helpers");

const feature = loadFeature("./features/user.feature");

defineFeature(feature, (test) => {
  let response;

  test("Create a new user", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I create a user with id "(.*)"$/, async (id) => {
      response = await api.post("/user/", makeUser(id));
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get user by ID", ({ given, when, then, and }) => {
    given(/^a user with id "(.*)" exists$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    when(/^I get the user with id "(.*)"$/, async (id) => {
      response = await api.get(`/user/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the user name should be "(.*)"$/, (name) => {
      expect(response.data.data.nama_lengkap).toBe(name);
    });
  });

  test("List all users", ({ given, when, then, and }) => {
    given(/^a user with id "(.*)" exists$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    when("I list all users", async () => {
      response = await api.get("/user/");
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and("the data should be a non-empty list", () => {
      expect(Array.isArray(response.data.data)).toBe(true);
      expect(response.data.data.length).toBeGreaterThan(0);
    });
  });

  test("Update an existing user", ({ given, when, then, and }) => {
    given(/^a user with id "(.*)" exists$/, async (id) => {
      await api.delete(`/user/${id}`);
      await api.post("/user/", makeUser(id));
    });

    when(/^I update user "(.*)" name to "(.*)"$/, async (id, name) => {
      const user = makeUser(id);
      user.nama_lengkap = name;
      response = await api.put(`/user/${id}`, user);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Delete a user", ({ given, when, then, and }) => {
    given(/^a user with id "(.*)" exists$/, async (id) => {
      await api.post("/user/", makeUser(id));
    });

    when(/^I delete the user with id "(.*)"$/, async (id) => {
      response = await api.delete(`/user/${id}`);
    });

    then("the response should be successful", () => {
      expect(response.data.success).toBe(true);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });

  test("Get non-existent user", ({ given, when, then, and }) => {
    given("the API server is running", () => {});

    when(/^I get the user with id "(.*)"$/, async (id) => {
      response = await api.get(`/user/${id}`);
    });

    then("the response should not be successful", () => {
      expect(response.data.success).toBe(false);
    });

    and(/^the message should be "(.*)"$/, (msg) => {
      expect(response.data.message).toBe(msg);
    });
  });
});
