from pathlib import Path

from app.inference.schema import infer_schema
from app.mapping.suggest import apply_llm_suggestions, suggest_mapping
from app.readers import read_sample


def _suggest(path: Path, fmt: str) -> dict:
    recs = read_sample(path, fmt, 100)
    return suggest_mapping(infer_schema(recs), recs, "transaction")


def test_indonesian_csv_mapping(fixtures: Path) -> None:
    res = _suggest(fixtures / "transaksi_id.csv", "csv")
    m = res["suggested_mapping"]
    ev, cu = m["event"], m["customer"]
    assert ev["external_id"]["from"] == "id_transaksi"
    assert ev["occurred_at"] == {
        "from": "tgl_transaksi",
        "transform": [{"fn": "parse_datetime", "format": "%d/%m/%Y %H:%M", "timezone": "Asia/Jakarta"}],
    }
    assert ev["customer_external_id"]["from"] == "id_pelanggan"
    assert ev["amount"] == {"from": "nominal", "transform": [{"fn": "to_number", "locale": "id"}]}
    assert ev["instrument_fingerprint"] == {"from": "no_kartu", "transform": [{"fn": "hash_pan"}]}
    assert ev["card_bin"]["transform"][0]["fn"] == "pan_bin"
    assert ev["card_last4"]["transform"][0]["fn"] == "pan_last4"
    assert ev["promo_code"]["from"] == "kode_voucher"
    assert ev["ip_address"]["from"] == "alamat_ip"
    assert cu["phone"]["from"] == "no_hp" and cu["phone"]["transform"][0]["fn"] == "normalize_phone"
    assert cu["email"]["from"] == "email"
    assert cu["full_name"]["from"] == "nama_pelanggan"
    assert set(m["drop_fields"]) >= {"no_kartu", "cvv"}
    et = m["event_type"]
    assert et["from"] == "jenis_transaksi"
    assert et["value_map"]["PEMBELIAN"] == "transaction"
    assert et["value_map"]["LOGIN"] == "login"
    assert et["value_map"]["VOUCHER"] == "promo_redemption"
    assert m["label"]["from"] == "is_fraud"
    assert res["confidence"]["event.instrument_fingerprint"] >= 0.9
    assert not any("required" in n for n in res["notes"])


def test_nested_json_mapping(fixtures: Path) -> None:
    res = _suggest(fixtures / "events_nested.jsonl", "jsonl")
    ev, cu = res["suggested_mapping"]["event"], res["suggested_mapping"]["customer"]
    assert ev["external_id"]["from"] == "order_id"
    assert ev["occurred_at"]["transform"] == [{"fn": "parse_datetime", "format": "rfc3339"}]
    assert ev["customer_external_id"]["from"] == "user.id"
    assert ev["customer_external_id"]["transform"] == [{"fn": "to_string"}]
    assert ev["amount"]["from"] == "total"
    assert ev["device_id"]["from"] == "device.id"
    assert ev["ip_address"]["from"] == "ip"
    assert ev["promo_code"]["from"] == "promo"
    assert cu["email"]["from"] == "user.email"
    assert cu["phone"]["from"] == "user.hp"
    assert res["suggested_mapping"]["event_type"] == {"const": "transaction"}
    assert "label" not in res["suggested_mapping"]


def test_missing_required_is_reported() -> None:
    recs = [{"foo": "x", "bar": 1}, {"foo": "y", "bar": 2}]
    res = suggest_mapping(infer_schema(recs), recs)
    assert any("occurred_at" in n for n in res["notes"])
    assert set(res["unmapped_fields"]) == {"foo", "bar"}


def test_llm_suggestions_merge_only_free_slots() -> None:
    recs = [{"kolom_aneh": "ABC-1", "id": "1", "created_at": "2026-09-01T00:00:00Z"}]
    fields = infer_schema(recs)
    res = suggest_mapping(fields, recs)
    res = apply_llm_suggestions(
        res,
        fields,
        [
            {
                "source_path": "kolom_aneh",
                "target": "event.merchant_id",
                "confidence": 0.8,
                "reason": "merchant code",
            },
            {"source_path": "kolom_aneh", "target": "event.not_a_field", "confidence": 0.99},
        ],
    )
    assert res["suggested_mapping"]["event"]["merchant_id"]["from"] == "kolom_aneh"
    assert "kolom_aneh" not in res["unmapped_fields"]


def test_hashed_sources_always_dropped_and_pengguna_context() -> None:
    recs = [
        {
            "no_ref": f"R{i}",
            "waktu": "01/09/2026 08:15:22",
            "pengguna": {"id": f"U{i}", "email": "a@b.co"},
            "pembayaran": {"no_kartu": None, "rekening_tujuan": None},
            "nominal": "10.000,00",
        }
        for i in range(5)
    ]
    res = suggest_mapping(infer_schema(recs), recs)
    ev = res["suggested_mapping"]["event"]
    assert ev["external_id"]["from"] == "no_ref"
    assert ev["customer_external_id"]["from"] == "pengguna.id"
    drops = set(res["suggested_mapping"].get("drop_fields", []))
    assert {"pembayaran.no_kartu", "pembayaran.rekening_tujuan"} <= drops
