"""The simulator's *custom* (non-canonical) record shape and the mapping that teaches the platform to read it.

The shape deliberately uses Indonesian field names, a nested user object, Indonesian number/date formats
and raw card / account numbers (which the mapping hashes and drops) — i.e. exactly the situation the
pluggable data-source feature exists for (docs/technical/data-sources.md).

Example record:
{
  "no_ref": "CHK-00000042", "waktu": "01/09/2026 08:15:22", "jenis": "PEMBELIAN", "status": "BERHASIL",
  "pengguna": {"id": "CHK-U00017", "nama": "Budi Santoso", "email": "budi.s@contoh.id",
               "no_hp": "081234567890",
               "tgl_daftar": "2026-03-01", "kyc": 2, "segmen": "regular", "penghasilan": 8500000},
  "nominal": "1.500.000,00", "mata_uang": "IDR", "kanal": "mobile_app",
  "merchant": {"id": "M-0012", "kategori": "5411"},
  "pembayaran": {"metode": "card", "no_kartu": "4111 1111 1111 1111", "negara_penerbit": "ID",
                 "rekening_tujuan": null},
  "perangkat": {"id": "dev-8f2a", "ip": "36.72.10.4", "negara": "ID", "kota": "Jakarta", "ua": "Android 14"},
  "voucher": {"kode": null, "diskon": 0, "cashback": 0},
  "pengiriman": {"alamat": "Jl. Melati No. 5, Menteng, Jakarta"}, "penagihan": {"alamat": "..."},
  "perubahan_akun": null, "login_berhasil": null, "id_klien_api": null, "ref_transaksi": null
}
"""

from __future__ import annotations

from typing import Any

JENIS = {
    "transaction": "PEMBELIAN",
    "login": "LOGIN",
    "account_change": "UBAH_AKUN",
    "promo_redemption": "KLAIM_VOUCHER",
    "payout": "TARIK_DANA",
    "registration": "DAFTAR",
    "refund": "RETUR",
    "transfer": "TRANSFER",
}

DATETIME_FORMAT = "%d/%m/%Y %H:%M:%S"
TIMEZONE = "Asia/Jakarta"


def _f(path: str, *transform: dict[str, Any]) -> dict[str, Any]:
    spec: dict[str, Any] = {"from": path}
    if transform:
        spec["transform"] = list(transform)
    return spec


def mapping() -> dict[str, Any]:
    """Mapping JSON (data-sources.md §3) for the custom shape."""
    value_map = {v: k for k, v in JENIS.items()}
    value_map["TRANSFER"] = "transaction"
    return {
        "event_type": {"from": "jenis", "value_map": value_map, "default": "transaction"},
        "event": {
            "external_id": _f("no_ref"),
            "occurred_at": _f(
                "waktu", {"fn": "parse_datetime", "format": DATETIME_FORMAT, "timezone": TIMEZONE}
            ),
            "customer_external_id": _f("pengguna.id", {"fn": "to_string"}),
            "amount": _f("nominal", {"fn": "to_number", "locale": "id"}),
            "currency": _f("mata_uang", {"fn": "uppercase"}),
            "channel": _f("kanal"),
            "status": _f("status", {"fn": "lowercase"}),
            "merchant_id": _f("merchant.id"),
            "merchant_category": _f("merchant.kategori"),
            "payment_method": _f("pembayaran.metode"),
            "instrument_fingerprint": _f("pembayaran.no_kartu", {"fn": "hash_pan"}),
            "card_bin": _f("pembayaran.no_kartu", {"fn": "pan_bin", "length": 6}),
            "card_last4": _f("pembayaran.no_kartu", {"fn": "pan_last4"}),
            "issuer_country": _f("pembayaran.negara_penerbit", {"fn": "uppercase"}),
            "recipient_fingerprint": _f("pembayaran.rekening_tujuan", {"fn": "hash_account"}),
            "device_id": _f("perangkat.id"),
            "ip_address": _f("perangkat.ip"),
            "geo_country": _f("perangkat.negara", {"fn": "uppercase"}),
            "geo_city": _f("perangkat.kota"),
            "user_agent": _f("perangkat.ua"),
            "promo_code": _f("voucher.kode"),
            "discount_amount": _f("voucher.diskon", {"fn": "to_number"}),
            "cashback_amount": _f("voucher.cashback", {"fn": "to_number"}),
            "ref_transaction_id": _f("ref_transaksi"),
            "shipping_address": _f("pengiriman.alamat"),
            "billing_address": _f("penagihan.alamat"),
            "account_change_type": _f("perubahan_akun"),
            "login_success": _f("login_berhasil", {"fn": "to_bool"}),
            "api_client_id": _f("id_klien_api"),
        },
        "customer": {
            "full_name": _f("pengguna.nama"),
            "email": _f("pengguna.email", {"fn": "normalize_email"}),
            "phone": _f("pengguna.no_hp", {"fn": "normalize_phone", "default_country": "ID"}),
            "registered_at": _f(
                "pengguna.tgl_daftar", {"fn": "parse_datetime", "format": "%Y-%m-%d", "timezone": TIMEZONE}
            ),
            "kyc_level": _f("pengguna.kyc", {"fn": "to_number"}),
            "segment": _f("pengguna.segmen"),
            "attributes": {"monthly_income": _f("pengguna.penghasilan", {"fn": "to_number"})},
        },
        "drop_fields": ["pembayaran.no_kartu", "pembayaran.rekening_tujuan", "pembayaran.cvv"],
    }


def fmt_amount(x: float | None) -> str | None:
    """Indonesian number format: 1.500.000,00"""
    if x is None:
        return None
    whole, frac = f"{x:,.2f}".split(".")
    return f"{whole.replace(',', '.')},{frac}"
