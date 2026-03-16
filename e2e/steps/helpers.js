const axios = require("axios");

const BASE_URL = process.env.API_BASE_URL || "http://localhost:8000/api/v1";

const api = axios.create({
  baseURL: BASE_URL,
  timeout: 10000,
  validateStatus: () => true, // Don't throw on non-2xx
});

function makeUser(id_user) {
  return {
    id_user,
    nama_lengkap: "BDD Test User",
    email: `${id_user}@example.com`,
    domain_email: "example.com",
    address: "Jl. BDD Test",
    address_zip: "12345",
    address_city: "Jakarta",
    address_province: "DKI",
    address_kecamatan: "Menteng",
    phone_number: "081234567890",
  };
}

function makeTransaction(id_transaction, amount = 200000) {
  return {
    id_transaction,
    id_user: "bdd_user",
    shipzip: "40000",
    shipping_address: "Jl. BDD Shipping",
    shipping_city: "Jakarta",
    shipping_province: "DKI Jakarta",
    shipping_kecamatan: "Menteng",
    payment_type: "credit_card",
    number: "1234567890",
    bank_name: "BCA",
    amount,
    status: "success",
    billing_address: "Jl. BDD Billing",
    billing_city: "Jakarta",
    billing_province: "DKI Jakarta",
    billing_kecamatan: "Menteng",
    list_of_items: [{ item_id: "item1", qty: 1, price: amount }],
  };
}

function makeStandardRule(risk_point = 50, field = "amount", operator = ">", value = 100000) {
  return {
    description: `${field} ${operator} ${value}`,
    risk_point,
    rule_type: "standard",
    field,
    operator,
    value,
  };
}

function makeVelocityRule(risk_point = 40, field = "id_user", time_range = "1h", threshold = 5) {
  return {
    description: `Velocity: ${field} in ${time_range}`,
    risk_point,
    rule_type: "velocity",
    field,
    time_range,
    aggregation_function: "count",
    threshold,
  };
}

function makePolicy(policy_id, rules = []) {
  return {
    policy_id,
    name: "BDD Policy",
    description: "BDD test policy",
    rules,
  };
}

module.exports = { api, makeUser, makeTransaction, makeStandardRule, makeVelocityRule, makePolicy };
