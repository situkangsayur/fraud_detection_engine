Feature: Policy Management
  As a fraud analyst
  I want to manage fraud detection policies
  So that I can group rules for transaction evaluation

  Scenario: Create a new policy
    Given the API server is running
    When I create a policy with id "bdd_policy_001"
    Then the response should be successful
    And the message should be "Policy created"

  Scenario: Get policy by ID
    Given a policy with id "bdd_policy_002" exists
    When I get the policy with id "bdd_policy_002"
    Then the response should be successful
    And the policy name should be "BDD Policy"

  Scenario: List all policies
    Given a policy with id "bdd_policy_003" exists
    When I list all policies
    Then the response should be successful
    And the data should be a non-empty list

  Scenario: Delete a policy
    Given a policy with id "bdd_policy_004" exists
    When I delete the policy with id "bdd_policy_004"
    Then the response should be successful
    And the message should be "Policy deleted"

  Scenario: Get non-existent policy
    Given the API server is running
    When I get the policy with id "nonexistent_policy"
    Then the response should not be successful
    And the message should be "Policy not found"
