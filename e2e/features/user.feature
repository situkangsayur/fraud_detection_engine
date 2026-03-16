Feature: User Management
  As a fraud analyst
  I want to manage user profiles
  So that I can track user activities

  Scenario: Create a new user
    Given the API server is running
    When I create a user with id "bdd_user_001"
    Then the response should be successful
    And the message should be "User created"

  Scenario: Get user by ID
    Given a user with id "bdd_user_002" exists
    When I get the user with id "bdd_user_002"
    Then the response should be successful
    And the user name should be "BDD Test User"

  Scenario: List all users
    Given a user with id "bdd_user_003" exists
    When I list all users
    Then the response should be successful
    And the data should be a non-empty list

  Scenario: Update an existing user
    Given a user with id "bdd_user_004" exists
    When I update user "bdd_user_004" name to "Updated BDD User"
    Then the response should be successful
    And the message should be "User updated"

  Scenario: Delete a user
    Given a user with id "bdd_user_005" exists
    When I delete the user with id "bdd_user_005"
    Then the response should be successful
    And the message should be "User deleted"

  Scenario: Get non-existent user
    Given the API server is running
    When I get the user with id "nonexistent_user"
    Then the response should not be successful
    And the message should be "User not found"
