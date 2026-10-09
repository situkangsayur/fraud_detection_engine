"""Public dataset loaders → simulator `Dataset` objects (custom record shape + ground truth).

Every loader samples deterministically (seed) so that runs are reproducible, shifts the dataset's own timeline so
it ends at `end` (relative spacing is kept), and stores the dataset's label as ground truth (`fraud_type`).
Columns that the canonical mapping does not use are kept in the record, so they reach the platform as `source.*`
fields (available to rules and to the research export via features/payload).

Sources (see docs/technical/research-dataset.md for licences):
  paysim   https://huggingface.co/datasets/theman10/paysim                          (MIT)
  sparkov  https://huggingface.co/datasets/santosh3110/credit_card_fraud_transactions (Apache-2.0 tag; Kaggle
           "fraudTrain" by kartik2112, truncated to 1,048,575 rows)
  saml-d   https://huggingface.co/datasets/LordNR/AMLGraphX-SAML-D                  (CC BY-NC-SA 4.0)
"""

from __future__ import annotations

import hashlib
import zipfile
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd
from simulator.scenarios import Dataset, ProjectSpec
from simulator.shape import fmt_amount
from simulator.world import WIB, SimEvent

DATETIME_FORMAT = "%d/%m/%Y %H:%M:%S"


@dataclass(frozen=True)
class Source:
    name: str
    url: str
    file: str  # path inside ~/datasets/fraud-public/<name>/
    license: str
    loader: Callable[[Path, int, int, datetime], ResearchDataset]


def _record(
    ext: str,
    ts: datetime,
    jenis: str,
    customer: dict[str, Any],
    amount: float,
    *,
    currency: str = "USD",
    kanal: str = "web",
    merchant: tuple[str | None, str | None] = (None, None),
    method: str | None = None,
    card: str | None = None,
    recipient: str | None = None,
    issuer_country: str | None = None,
    country: str | None = None,
    city: str | None = None,
    address: str | None = None,
    extra: dict[str, Any] | None = None,
) -> dict[str, Any]:
    rec: dict[str, Any] = {
        "no_ref": ext,
        "waktu": ts.astimezone(WIB).strftime(DATETIME_FORMAT),
        "jenis": jenis,
        "status": "BERHASIL",
        "pengguna": {"tgl_daftar": None, "kyc": 1, "segmen": "regular", "penghasilan": None, **customer},
        "nominal": fmt_amount(round(float(amount), 2)),
        "mata_uang": currency,
        "kanal": kanal,
        "merchant": {"id": merchant[0], "kategori": merchant[1]},
        "pembayaran": {
            "metode": method,
            "no_kartu": card,
            "negara_penerbit": issuer_country,
            "rekening_tujuan": recipient,
        },
        "perangkat": {"id": None, "ip": None, "negara": country, "kota": city, "ua": None},
        "voucher": {"kode": None, "diskon": 0, "cashback": 0},
        "pengiriman": {"alamat": address},
        "penagihan": {"alamat": address},
        "perubahan_akun": None,
        "login_berhasil": None,
        "id_klien_api": None,
        "ref_transaksi": None,
    }
    if extra:
        rec["sumber"] = extra  # dataset-specific columns → source.sumber.*
    return rec


def _digits16(text: str) -> str:
    """Stable 16-digit pseudo card number (sha256, unlike hash() which is randomised per process)."""
    return str(int(hashlib.sha256(text.encode()).hexdigest(), 16) % 10**16).zfill(16)


def _shift(ts: pd.Series, end: datetime) -> pd.Series:
    """Move a timeline so its last event is at `end`, keeping relative spacing."""
    ts = pd.to_datetime(ts, utc=True)
    return ts + (pd.Timestamp(end) - ts.max())


@dataclass
class ResearchDataset(Dataset):
    """`events[].fraud_type` is the platform typology sent as label (carding, money_mule, …); `truth_detail` keeps the
    dataset's own fine-grained label per external_id for evaluation only — it is never sent to the platform."""

    truth_detail: dict[str, str] = field(default_factory=dict)


def _dataset(spec: ProjectSpec, events: list[SimEvent], detail: dict[str, str]) -> ResearchDataset:
    events.sort(key=lambda e: e.ts)
    return ResearchDataset(project=spec, events=events, customers=[], fraud_customers={}, truth_detail=detail)


# --------------------------------------------------------------------------------------------- Sparkov
SPARKOV = ProjectSpec(
    "rs-sparkov-card",
    "Research — Sparkov card purchases",
    "pre_payment",
    "SPK",
    1.0,
    "Public dataset (Sparkov fraudTrain, simulated US card transactions)",
    "Card-not-present and card-present purchases; fraud = card fraud (stolen/compromised cards).",
)


def load_sparkov(folder: Path, target: int, seed: int, end: datetime) -> ResearchDataset:
    with zipfile.ZipFile(folder / "credit_card_fraud_transactions.zip") as z:
        name = next(n for n in z.namelist() if n.endswith(".csv"))
        df = pd.read_csv(z.open(name), dtype={"cc_num": str, "zip": str})
    # This mirror went through Excel: trans_date_trans_time lost its seconds and cc_num is rounded to 6 significant
    # digits. unix_time is still exact, and card identity is rebuilt from the rounded number + holder + birth date.
    df["ts"] = pd.to_datetime(df["unix_time"], unit="s", utc=True)
    df["card_key"] = (
        df["cc_num"].str.replace(".0", "", regex=False)
        + "-"
        + df["first"]
        + "-"
        + df["last"]
        + "-"
        + df["dob"].astype(str)
    ).map(_digits16)
    # last window of the timeline for *all* cards keeps per-card history (velocity) intact
    df = df.sort_values("ts")
    df = df.iloc[-target:] if len(df) > target else df
    df["ts"] = _shift(df["ts"], end)
    events: list[SimEvent] = []
    detail: dict[str, str] = {}
    for r in df.itertuples(index=False):
        cust = {
            "id": f"SPK-C{r.card_key}",
            "nama": f"{r.first} {r.last}",
            "email": None,
            "no_hp": None,
            "segmen": str(r.job)[:40],
        }
        rec = _record(
            f"SPK-{r.trans_num}",
            r.ts.to_pydatetime(),
            "PEMBELIAN",
            cust,
            r.amt,
            kanal="other" if str(r.category).endswith("_pos") else "web",  # card-present
            merchant=(str(r.merchant), str(r.category)),
            method="card",
            card="4" + str(r.card_key)[1:],  # Visa-like 16 digits so pan_bin/pan_last4/hash_pan behave as for real PANs
            issuer_country="US",
            country="US",
            city=str(r.city),
            address=f"{r.street}, {r.city}, {r.state} {r.zip}",
            extra={
                "city_pop": int(r.city_pop),
                "merch_lat": float(r.merch_lat),
                "merch_long": float(r.merch_long),
                "lat": float(r.lat),
                "long": float(r.long),
                "gender": r.gender,
                "dob": r.dob,
            },
        )
        if r.is_fraud:
            detail[rec["no_ref"]] = "card_fraud"
        events.append(SimEvent(r.ts.to_pydatetime(), rec, cust["id"], "carding" if r.is_fraud else None))
    return _dataset(SPARKOV, events, detail)


# --------------------------------------------------------------------------------------------- PaySim
PAYSIM = ProjectSpec(
    "rs-paysim-mobile-money",
    "Research — PaySim mobile money",
    "payout",
    "PSM",
    1.0,
    "Public dataset (PaySim, simulated mobile-money transactions)",
    "Account takeover followed by TRANSFER to a mule and CASH_OUT.",
)
PAYSIM_JENIS = {
    "TRANSFER": "TRANSFER",
    "CASH_OUT": "TARIK_DANA",
    "PAYMENT": "PEMBELIAN",
    "CASH_IN": "TRANSFER",
    "DEBIT": "TARIK_DANA",
}


def load_paysim(folder: Path, target: int, seed: int, end: datetime) -> ResearchDataset:
    df = pd.read_csv(folder / "paysim.csv")
    fraud = df[df["isFraud"] == 1]
    n_fraud = min(len(fraud), max(1, int(target * 0.02)))  # ~2 % fraud rate in the sample
    legit = df[df["isFraud"] == 0].sample(n=min(target - n_fraud, int((df["isFraud"] == 0).sum())), random_state=seed)
    df = pd.concat([fraud.sample(n=n_fraud, random_state=seed), legit])
    rng = np.random.default_rng(seed)
    base = datetime(2026, 1, 1, tzinfo=UTC)
    # step = simulated hour; spread events inside the hour deterministically
    df["ts"] = [
        base + timedelta(hours=int(s), seconds=int(x))
        for s, x in zip(df["step"], rng.integers(0, 3600, len(df)), strict=True)
    ]
    df["ts"] = _shift(df["ts"], end)
    events: list[SimEvent] = []
    detail: dict[str, str] = {}
    for i, r in enumerate(df.itertuples(index=False)):
        cust = {"id": f"PSM-{r.nameOrig}", "nama": None, "email": None, "no_hp": None}
        merchant = (str(r.nameDest), "mobile_money") if str(r.nameDest).startswith("M") else (None, None)
        rec = _record(
            f"PSM-{i:08d}",
            r.ts.to_pydatetime(),
            PAYSIM_JENIS.get(r.type, "TRANSFER"),
            cust,
            r.amount,
            currency="USD",
            kanal="mobile_app",
            merchant=merchant,
            method="ewallet",
            recipient=None if merchant[0] else str(r.nameDest),
            extra={
                "type": r.type,
                "oldbalance_org": float(r.oldbalanceOrg),
                "newbalance_orig": float(r.newbalanceOrig),
                "oldbalance_dest": float(r.oldbalanceDest),
                "newbalance_dest": float(r.newbalanceDest),
            },
        )
        if r.isFraud:
            detail[rec["no_ref"]] = f"ato_{str(r.type).lower()}"
        events.append(SimEvent(r.ts.to_pydatetime(), rec, cust["id"], "bank_account_takeover" if r.isFraud else None))
    return _dataset(PAYSIM, events, detail)


# --------------------------------------------------------------------------------------------- SAML-D
SAMLD = ProjectSpec(
    "rs-samld-aml",
    "Research — SAML-D anti money laundering",
    "payout",
    "AML",
    1.0,
    "Public dataset (SAML-D, synthetic AML transactions with 28 typologies)",
    "Money laundering / money mule transfers; typology given per transaction.",
)


SAMLD_CURRENCY = {
    "UK pounds": "GBP",
    "Euro": "EUR",
    "Turkish lira": "TRY",
    "Swiss franc": "CHF",
    "Dirham": "AED",
    "Pakistani rupee": "PKR",
    "Naira": "NGN",
    "US dollar": "USD",
    "Yen": "JPY",
    "Moroccan dirham": "MAD",
    "Mexican Peso": "MXN",
    "Albanian lek": "ALL",
    "Indian rupee": "INR",
}


def load_samld(folder: Path, target: int, seed: int, end: datetime) -> ResearchDataset:
    with zipfile.ZipFile(folder / "SAML-D.zip") as z:
        name = next(n for n in z.namelist() if n.lower().endswith(".csv") and not n.startswith("__MACOSX"))
        df = pd.read_csv(z.open(name))
    df["ts"] = pd.to_datetime(df["Date"].astype(str) + " " + df["Time"].astype(str))
    senders_pos = df.loc[df["Is_laundering"] == 1, "Sender_account"].unique()
    rng = np.random.default_rng(seed)
    per_sender = df.groupby("Sender_account").size()
    # ~30 % of the sampled rows come from accounts that launder (all of their transactions are kept), so the 28
    # typologies have enough positives; the event-level fraud rate stays low because launderers also transact normally
    pos = rng.permutation(senders_pos)
    chosen, rows = [], 0
    for acc in pos:
        if rows >= target * 0.30:
            break
        chosen.append(acc)
        rows += int(per_sender[acc])
    neg = rng.permutation(per_sender.index.difference(senders_pos))
    for acc in neg:
        if rows >= target:
            break
        chosen.append(acc)
        rows += int(per_sender[acc])
    df = df[df["Sender_account"].isin(set(chosen))].copy()
    df["ts"] = _shift(df["ts"], end)
    events: list[SimEvent] = []
    detail: dict[str, str] = {}
    for i, r in enumerate(df.itertuples(index=False)):
        cust = {"id": f"AML-{r.Sender_account}", "nama": None, "email": None, "no_hp": None}
        rec = _record(
            f"AML-{i:08d}",
            r.ts.to_pydatetime(),
            "TRANSFER",
            cust,
            r.Amount,
            currency=SAMLD_CURRENCY.get(str(r.Payment_currency), "GBP"),
            kanal="web",
            method=str(r.Payment_type).lower(),
            recipient=str(r.Receiver_account),
            issuer_country=None,
            country=None,
            city=str(r.Sender_bank_location),
            extra={
                "receiver_bank_location": r.Receiver_bank_location,
                "received_currency": r.Received_currency,
                "payment_type": r.Payment_type,
            },
        )
        if r.Is_laundering == 1:
            detail[rec["no_ref"]] = str(r.Laundering_type)
        events.append(SimEvent(r.ts.to_pydatetime(), rec, cust["id"], "money_mule" if r.Is_laundering == 1 else None))
    return _dataset(SAMLD, events, detail)


SOURCES: dict[str, Source] = {
    "sparkov": Source(
        "sparkov",
        "https://huggingface.co/datasets/santosh3110/credit_card_fraud_transactions",
        "credit_card_fraud_transactions.zip",
        "Apache-2.0 (HF tag)",
        load_sparkov,
    ),
    "paysim": Source("paysim", "https://huggingface.co/datasets/theman10/paysim", "paysim.csv", "MIT", load_paysim),
    "saml-d": Source(
        "saml-d", "https://huggingface.co/datasets/LordNR/AMLGraphX-SAML-D", "SAML-D.zip", "CC BY-NC-SA 4.0", load_samld
    ),
}
