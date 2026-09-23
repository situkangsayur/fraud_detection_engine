from pathlib import Path

import pandas as pd

from app.inference.schema import infer_schema
from app.inference.types import guess_datetime_format, infer_type, parse_number
from app.readers import detect_format, iter_records, read_sample, sniff_delimiter


def _fields(records: list[dict]) -> dict[str, dict]:
    return {f["path"]: f for f in infer_schema(records)}


def test_parse_number_locales() -> None:
    assert parse_number("1.500.000,00") == 1_500_000.0
    assert parse_number("250.000,50") == 250_000.5
    assert parse_number("1,500,000.25") == 1_500_000.25
    assert parse_number("Rp 12.000") == 12_000.0
    assert parse_number("abc") is None


def test_datetime_formats() -> None:
    assert guess_datetime_format(["01/09/2026 08:15", "13/09/2026 23:59"]) == "%d/%m/%Y %H:%M"
    assert guess_datetime_format(["01-09-2026 08:15", "02-09-2026 09:00"]) == "%d-%m-%Y %H:%M"
    assert guess_datetime_format(["2026-09-01T08:15:00Z", "2026-09-01T09:00:00+07:00"]) == "rfc3339"
    assert infer_type("created_ts", [1_780_000_000, 1_780_000_500]) == ("datetime", "unix_s")
    assert infer_type("event_time", [1_780_000_000_000]) == ("datetime", "unix_ms")
    assert infer_type("amount", [1_780_000_000]) == ("integer", None)  # no time hint → a number


def test_csv_semicolon_indonesian(fixtures: Path) -> None:
    path = fixtures / "transaksi_id.csv"
    assert detect_format(path.name, path.read_bytes()[:100]) == "csv"
    assert sniff_delimiter(path.read_text()[:500]) == ";"
    records = read_sample(path, "csv", 100)
    assert len(records) == 5
    f = _fields(records)
    assert f["tgl_transaksi"]["inferred_type"] == "datetime"
    assert f["tgl_transaksi"]["datetime_format"] == "%d/%m/%Y %H:%M"
    assert f["nominal"]["inferred_type"] == "number"
    assert f["no_hp"]["inferred_type"] == "string" and f["no_hp"]["pii"] == "phone"
    assert f["no_kartu"]["pii"] == "pan"
    assert all("*" in s for s in f["no_kartu"]["sample_values"])  # samples are masked
    assert f["email"]["pii"] == "email"
    assert f["kode_voucher"]["null_ratio"] == 0.6
    assert f["is_fraud"]["inferred_type"] == "integer"


def test_nested_jsonl(fixtures: Path) -> None:
    path = fixtures / "events_nested.jsonl"
    assert detect_format(path.name, path.read_bytes()[:200]) == "jsonl"
    f = _fields(read_sample(path, "jsonl", 10))
    assert f["user.id"]["inferred_type"] == "integer"
    assert f["user.email"]["pii"] == "email"
    assert f["created_at"]["datetime_format"] == "rfc3339"
    assert f["items[0].sku"]["inferred_type"] == "string"
    assert "items[]" in f
    assert f["total"]["inferred_type"] == "number"


def test_parquet_and_xlsx(tmp_path: Path) -> None:
    df = pd.DataFrame(
        {
            "trx_id": ["A", "B"],
            "amount": [10.5, 20.0],
            "user.id": ["u1", "u2"],
            "ts": pd.to_datetime(["2026-09-01 10:00", "2026-09-02 11:00"]),
        }
    )
    pq_path = tmp_path / "d.parquet"
    df.to_parquet(pq_path)
    recs = read_sample(pq_path, "parquet", 10)
    assert recs[0]["user"] == {"id": "u1"}  # dotted header un-flattened
    f = _fields(recs)
    assert f["amount"]["inferred_type"] == "number"
    assert f["ts"]["inferred_type"] == "datetime"
    xl = tmp_path / "d.xlsx"
    df.to_excel(xl, index=False)
    assert detect_format("d.xlsx", xl.read_bytes()[:10]) == "xlsx"
    recs = read_sample(xl, "xlsx", 10)
    assert recs[1]["trx_id"] == "B" and recs[1]["user"]["id"] == "u2"


def test_chunked_iteration(tmp_path: Path) -> None:
    p = tmp_path / "big.csv"
    p.write_text("a,b\n" + "".join(f"{i},{i * 2}\n" for i in range(1234)))
    sizes = [len(c) for c in iter_records(p, "csv", chunk_size=500)]
    assert sizes == [500, 500, 234]


def test_unflatten_null_parent_does_not_shadow_children() -> None:
    from app.readers import unflatten

    assert unflatten({"pengiriman": None, "pengiriman.alamat": "Jl. A"}) == {
        "pengiriman": {"alamat": "Jl. A"}
    }
    assert unflatten({"pengiriman.alamat": None, "pengiriman": None}) == {"pengiriman": {"alamat": None}}
    assert unflatten({"a": 1, "b.c": 2}) == {"a": 1, "b": {"c": 2}}
