Feature: Transaction Processing & Fraud Detection
  As a fraud detection system
  I want to evaluate transactions against rules
  So that I can detect fraudulent transactions

  Scenario: Process a high-amount transaction as fraud
    Given a user "proc_user_001" exists in the system
    And a standard rule with risk_point 90 for amount > 100000 exists
    And a transaction "proc_trx_001" with amount 200000 exists
    When I process the transaction "proc_trx_001"
    Then the response should be successful
    And the risk score should be at least 90
    And the detected status should be "fraud"
    And matched rules should not be empty

  Scenario: Process a transaction and get evaluated result
    Given a user "proc_user_002" exists in the system
    And a standard rule with risk_point 10 for amount > 500000 exists
    And a transaction "proc_trx_002" with amount 50000 exists
    When I process the transaction "proc_trx_002"
    Then the response should be successful
    And the detected status should be present

  Scenario: Process non-existent transaction
    Given the API server is running
    When I process the transaction "nonexistent_trx"
    Then the response should not be successful
    And the message should be "Transaction not found"

  Scenario: Process transaction with velocity rule
    Given a user "proc_user_003" exists in the system
    And a velocity rule with risk_point 50 exists
    And a transaction "proc_trx_003" with amount 100000 exists
    When I process the transaction "proc_trx_003"
    Then the response should be successful
    And the risk score should be at least 50
    And matched rules should not be empty
