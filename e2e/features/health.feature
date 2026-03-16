Feature: Health Check
  As a system administrator
  I want to verify the API is running
  So that I know the system is operational

  Scenario: API health check returns ok
    Given the API server is running
    When I request the health endpoint
    Then the response status should be "ok"
    And the database should be "connected"
