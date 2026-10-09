from __future__ import annotations

import csv
import json
from datetime import UTC, datetime
from pathlib import Path

import pytest

from simulator.export import export
from simulator.labels import plan_labels
from simulator.scenarios import generate_all
from simulator.shape import mapping
from simulator.world import luhn_complete

END = datetime(2026, 9, 20, tzinfo=UTC)


@pytest.fixture(scope="module")
def datasets():
    return {d.project.slug: d for d in generate_all(400, 45, 7, END)}


def _luhn_ok(digits: str) -> bool:
    total = 0
    for i, ch in enumerate(reversed(digits)):
        d = int(ch)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0


def test_deterministic() -> None:
    a = generate_all(150, 20, 99, END, ["checkout"])[0]
    b = generate_all(150, 20, 99, END, ["checkout"])[0]
    assert [e.record for e in a.events] == [e.record for e in b.events]
    c = generate_all(150, 20, 100, END, ["checkout"])[0]
    assert [e.record for e in a.events] != [e.record for e in c.events]


def test_each_project_has_its_typologies(datasets) -> None:
    expected = {
        "checkout": {"carding", "account_takeover", "system_breach"},
        "post-payment": {"bank_account_takeover", "money_mule"},
        "returns": {"refund_abuse"},
        "promo": {"promo_abuse"},
    }
    for slug, types in expected.items():
        assert set(datasets[slug].typology_counts()) == types, slug


def test_fraud_rate_realistic(datasets) -> None:
    for ds in datasets.values():
        rate = sum(ds.typology_counts().values()) / len(ds.events)
        assert 0.001 < rate < 0.08, (ds.project.slug, rate)


def test_chronological_and_unique_refs(datasets) -> None:
    for ds in datasets.values():
        ts = [e.ts for e in ds.events]
        assert ts == sorted(ts)
        refs = [e.record["no_ref"] for e in ds.events]
        assert len(set(refs)) == len(refs)
        assert all(ds.project.prefix in r for r in refs)
        assert all(e.ts <= END for e in ds.events)


def test_no_ground_truth_leak(datasets) -> None:
    fraud_words = ("fraud", "carding", "takeover", "mule", "abuse", "breach")
    for ds in datasets.values():
        for e in ds.events[:3000]:
            blob = json.dumps(e.record).lower()
            assert not any(w in blob for w in fraud_words)
            assert e.record["pengguna"]["id"].startswith(f"{ds.project.prefix}-U")


def test_pans_are_luhn_valid_and_custom_shape(datasets) -> None:
    pans = [
        e.record["pembayaran"]["no_kartu"]
        for e in datasets["checkout"].events
        if e.record["pembayaran"] and e.record["pembayaran"]["no_kartu"]
    ]
    assert pans and all(_luhn_ok(p.replace(" ", "")) for p in pans[:500])
    assert _luhn_ok(luhn_complete("461700", 16, __import__("random").Random(1)))
    rec = datasets["checkout"].events[10].record
    assert "," in (rec["nominal"] or "0,00")  # Indonesian number format
    assert rec["waktu"][2] == "/"  # dd/mm/yyyy


def test_mapping_covers_shape(datasets) -> None:
    m = mapping()
    used = {spec["from"].split(".")[0] for spec in m["event"].values() if "from" in spec}
    used |= {spec["from"].split(".")[0] for k, spec in m["customer"].items() if k != "attributes"}
    used.add(m["event_type"]["from"])
    rec_keys = set(datasets["checkout"].events[0].record)
    assert rec_keys - used <= {"alasan_retur"}
    assert set(m["event_type"]["value_map"].values()) >= {
        "transaction",
        "login",
        "account_change",
        "promo_redemption",
        "payout",
        "registration",
        "refund",
    }
    assert "pembayaran.no_kartu" in m["drop_fields"]


def test_refunds_reference_earlier_orders(datasets) -> None:
    ds = datasets["returns"]
    by_ref = {e.record["no_ref"]: e for e in ds.events}
    refunds = [e for e in ds.events if e.record["jenis"] == "RETUR"]
    assert refunds
    for r in refunds:
        orig = by_ref[r.record["ref_transaksi"]]
        assert orig.record["jenis"] == "PEMBELIAN" and orig.ts < r.ts


def test_promo_farm_shares_devices(datasets) -> None:
    ds = datasets["promo"]
    farm = [e for e in ds.events if e.fraud_type == "promo_abuse" and e.record["jenis"] == "KLAIM_VOUCHER"]
    devices = {e.record["perangkat"]["id"] for e in farm}
    customers = {e.record["pengguna"]["id"] for e in farm}
    assert len(customers) >= 2 * len(devices)  # many accounts per device


def test_label_plan(datasets) -> None:
    ds = datasets["checkout"]
    ev, cust = plan_labels(ds, 7)
    fraud_labels = [x for x in ev if x.label == "fraud"]
    eligible = sum(
        1
        for e in ds.events
        if e.fraud_type and e.ts <= ds.events[-1].ts.replace() and (ds.events[-1].ts - e.ts).days >= 7
    )
    assert 0.4 * eligible <= len(fraud_labels) <= 0.8 * eligible
    assert any(x.label == "legit" for x in ev)
    assert cust and all(c.external_id.startswith("CHK-U") for c in cust)
    newest = max(e.ts for e in ds.events)
    by_ref = {e.record["no_ref"]: e for e in ds.events}
    assert all((newest - by_ref[x.external_id].ts).days >= 7 for x in ev)  # delayed labels only


def test_export_csv_and_jsonl(datasets, tmp_path: Path) -> None:
    ds = datasets["returns"]
    n = export(ds, tmp_path / "r.csv", "csv", ";")
    with (tmp_path / "r.csv").open(encoding="utf-8") as fh:
        rows = list(csv.DictReader(fh, delimiter=";"))
    assert n == len(rows) == len(ds.events)
    assert "pengguna.id" in rows[0] and "is_fraud" in rows[0]
    assert sum(int(r["is_fraud"]) for r in rows) == sum(ds.typology_counts().values())
    export(ds, tmp_path / "r.jsonl", "jsonl")
    first = json.loads((tmp_path / "r.jsonl").read_text().splitlines()[0])
    assert isinstance(first["pengguna"], dict)
