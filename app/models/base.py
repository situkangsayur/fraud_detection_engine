# model_base.py
from pydantic import BaseModel, Field, ConfigDict
from typing import List, Optional, Union, Any, TypeVar, Generic, TypedDict
from enum import Enum
from bson import ObjectId


class RuleType(str, Enum):
    STANDARD = "standard"
    VELOCITY = "velocity"


class BaseConfig(BaseModel):
    model_config = ConfigDict(json_encoders={ObjectId: str})

T = TypeVar('T')

class ResponseModel(BaseConfig, Generic[T]):
    success: bool
    message: str
    data: Optional[T] = None


class ProcessedTransactionData(TypedDict):
    risk_score: int
    detected_status: str
    matched_rules: List[str]

class BaseRule(BaseConfig):
    rule_type: RuleType
    description: str
    risk_point: int


class StandardRule(BaseRule):
    rule_type: RuleType = RuleType.STANDARD
    field: str
    operator: str
    value: Any


class VelocityRule(BaseRule):
    rule_type: RuleType = RuleType.VELOCITY
    field: str
    time_range: str
    aggregation_function: str
    threshold: float


class Policy(BaseConfig):
    policy_id: str
    name: str
    description: str
    rules: List[Union[StandardRule, VelocityRule]]


class Transaction(BaseConfig):
    id_transaction: str
    id_user: str
    shipzip: str
    shipping_address: str
    shipping_city: str
    shipping_province: str
    shipping_kecamatan: str
    payment_type: str
    number: str
    bank_name: Optional[str]
    amount: float
    status: str
    billing_address: str
    billing_city: str
    billing_province: str
    billing_kecamatan: str
    list_of_items: List[dict]


class User(BaseConfig):
    id_user: str
    nama_lengkap: str
    email: str
    domain_email: str
    address: str
    address_zip: str
    address_city: str
    address_province: str
    address_kecamatan: str
    phone_number: str
