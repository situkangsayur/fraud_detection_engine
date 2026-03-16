import pytest
from app.models.base import (
    User,
    Transaction,
    Policy,
    StandardRule,
    VelocityRule,
    RuleType,
    ResponseModel,
    ProcessedTransactionData,
)


class TestModels:
    def test_standard_rule_defaults(self):
        rule = StandardRule(
            description="Test",
            risk_point=10,
            field="amount",
            operator=">",
            value=100,
        )
        assert rule.rule_type == RuleType.STANDARD

    def test_velocity_rule_defaults(self):
        rule = VelocityRule(
            description="Test",
            risk_point=10,
            field="id_user",
            time_range="1h",
            aggregation_function="count",
            threshold=5,
        )
        assert rule.rule_type == RuleType.VELOCITY

    def test_response_model_success(self):
        resp = ResponseModel(success=True, message="OK", data={"key": "value"})
        assert resp.success is True
        assert resp.data == {"key": "value"}

    def test_response_model_no_data(self):
        resp = ResponseModel(success=False, message="Not found")
        assert resp.data is None

    def test_user_model(self):
        user = User(
            id_user="u1",
            nama_lengkap="Test",
            email="test@example.com",
            domain_email="example.com",
            address="Jl. Test",
            address_zip="12345",
            address_city="Jakarta",
            address_province="DKI",
            address_kecamatan="Menteng",
            phone_number="081234567890",
        )
        assert user.id_user == "u1"
        d = user.model_dump()
        assert d["email"] == "test@example.com"

    def test_transaction_model(self):
        trx = Transaction(
            id_transaction="trx1",
            id_user="u1",
            shipzip="12345",
            shipping_address="Addr",
            shipping_city="City",
            shipping_province="Prov",
            shipping_kecamatan="Kec",
            payment_type="credit_card",
            number="123",
            bank_name=None,
            amount=100.0,
            status="success",
            billing_address="Addr",
            billing_city="City",
            billing_province="Prov",
            billing_kecamatan="Kec",
            list_of_items=[],
        )
        assert trx.bank_name is None
        assert trx.amount == 100.0

    def test_policy_model_with_rules(self):
        rule = StandardRule(
            description="Test",
            risk_point=10,
            field="amount",
            operator=">",
            value=100,
        )
        policy = Policy(
            policy_id="p1",
            name="Policy 1",
            description="Desc",
            rules=[rule],
        )
        assert len(policy.rules) == 1
        assert policy.rules[0].rule_type == RuleType.STANDARD

    def test_rule_type_enum(self):
        assert RuleType.STANDARD.value == "standard"
        assert RuleType.VELOCITY.value == "velocity"
