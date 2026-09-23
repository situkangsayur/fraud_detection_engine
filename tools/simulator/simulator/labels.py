"""Ground-truth → label plan (what an operator / chargeback feed would realistically report).

* Fraud events are only known after a delay (chargebacks, customer reports): events newer than
  `delay_days` are never labelled.
* Only `fraud_fraction` (default 60 %) of the older fraud events get labelled → realistic partial labels.
* A small sample of legit events is labelled `legit` (analyst reviews) so supervised training has negatives
  with confirmed status.
* `customer_fraction` of fraud-ring customers are marked fraud (feeds graph proximity).
"""

from __future__ import annotations

import random
from dataclasses import dataclass
from datetime import timedelta

from simulator.scenarios import Dataset

PAYMENT_TYPES = {"PEMBELIAN", "TRANSFER", "TARIK_DANA", "KLAIM_VOUCHER"}


@dataclass(frozen=True)
class EventLabel:
    external_id: str
    label: str
    fraud_type: str | None
    source: str


@dataclass(frozen=True)
class CustomerLabel:
    external_id: str
    fraud_type: str


def plan_labels(
    ds: Dataset,
    seed: int,
    fraud_fraction: float = 0.6,
    legit_fraction: float = 0.03,
    customer_fraction: float = 0.7,
    delay_days: int = 7,
) -> tuple[list[EventLabel], list[CustomerLabel]]:
    rng = random.Random(f"{seed}:{ds.project.slug}:labels")
    cutoff = max(e.ts for e in ds.events) - timedelta(days=delay_days) if ds.events else None
    events: list[EventLabel] = []
    for e in ds.events:
        if cutoff is None or e.ts > cutoff:
            continue
        ext = str(e.record["no_ref"])
        if e.fraud_type:
            if rng.random() < fraud_fraction:
                src = (
                    "chargeback"
                    if e.record["jenis"] in PAYMENT_TYPES and e.fraud_type == "carding"
                    else "analyst"
                )
                events.append(EventLabel(ext, "fraud", e.fraud_type, src))
        elif rng.random() < legit_fraction:
            events.append(EventLabel(ext, "legit", None, "analyst"))
    by_key = {c.key: c for c in ds.customers}
    customers = [
        CustomerLabel(by_key[k].ext_id, t)
        for k, t in sorted(ds.fraud_customers.items())
        if rng.random() < customer_fraction
    ]
    return events, customers
