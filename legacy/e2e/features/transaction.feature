Feature: Transaction Management
  As a fraud analyst
  I want to manage transactions
  So that I can track and evaluate them for fraud

  Scenario: Create a new transaction
    Given the API server is running
    When I create a transaction with id "bdd_trx_001" and amount 150000
    Then the response should be successful
    And the message should be "Transaction created"

  Scenario: Get transaction by ID
    Given a transaction with id "bdd_trx_002" exists
    When I get the transaction with id "bdd_trx_002"
    Then the response should be successful
    And the transaction amount should be 200000

  Scenario: List all transactions
    Given a transaction with id "bdd_trx_003" exists
    When I list all transactions
    Then the response should be successful
    And the data should be a non-empty list

  Scenario: Delete a transaction
    Given a transaction with id "bdd_trx_004" exists
    When I delete the transaction with id "bdd_trx_004"
    Then the response should be successful
    And the message should be "Transaction deleted"

  Scenario: Get non-existent transaction
    Given the API server is running
    When I get the transaction with id "nonexistent_trx"
    Then the response should not be successful
    And the message should be "Transaction not found"
