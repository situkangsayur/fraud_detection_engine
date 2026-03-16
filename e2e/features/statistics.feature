Feature: Statistics & Analytics
  As a fraud analyst
  I want to view statistics
  So that I can monitor system performance

  Scenario: Get user statistics
    Given the system has seeded data
    When I request user statistics
    Then the response should be successful
    And the data should be a list

  Scenario: Get single user statistics
    Given the system has seeded data
    When I request statistics for user "stats_user_001"
    Then the response should be successful
    And the data should contain "id_user"

  Scenario: Get transaction statistics
    Given the system has seeded data
    When I request transaction statistics
    Then the response should be successful
    And the data should contain "total_transactions"

  Scenario: Get policies performance
    Given the system has seeded data
    When I request policies performance
    Then the response should be successful
    And the data should be a list

  Scenario: Get rules performance
    Given the system has seeded data
    When I request rules performance
    Then the response should be successful
    And the data should be a list
