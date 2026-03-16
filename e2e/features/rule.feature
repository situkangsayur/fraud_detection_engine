Feature: Rule Management
  As a fraud analyst
  I want to manage fraud detection rules
  So that I can define criteria for fraud detection

  Scenario: Create a standard rule
    Given the API server is running
    When I create a standard rule with field "amount" operator ">" and value 100000
    Then the response should be successful
    And the message should be "Standard Rule created"

  Scenario: Create a velocity rule
    Given the API server is running
    When I create a velocity rule with field "id_user" time_range "1h" and threshold 5
    Then the response should be successful
    And the message should be "Velocity Rule created"

  Scenario: List all rules
    Given a standard rule exists
    When I list all rules
    Then the response should be successful
    And the data should be a non-empty list

  Scenario: Get a rule by ID
    Given a standard rule exists
    When I get the rule by its ID
    Then the response should be successful
    And the rule description should be present

  Scenario: Delete a rule
    Given a standard rule exists
    When I delete the rule by its ID
    Then the response should be successful
    And the message should be "Rule deleted"
