"""Deterministic synthetic world: customers, devices, cards, addresses and the record builder."""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta, timezone
from typing import Any

from simulator.shape import DATETIME_FORMAT, JENIS, fmt_amount

WIB = timezone(timedelta(hours=7))  # Asia/Jakarta (no DST)

FIRST = [
    "Budi",
    "Siti",
    "Andi",
    "Dewi",
    "Agus",
    "Rina",
    "Joko",
    "Sri",
    "Eko",
    "Wati",
    "Hendra",
    "Putri",
    "Rudi",
    "Nur",
    "Dian",
    "Fajar",
    "Indah",
    "Yusuf",
    "Ayu",
    "Bambang",
    "Lestari",
    "Rizky",
    "Maya",
    "Taufik",
    "Intan",
    "Arif",
    "Fitri",
    "Dimas",
    "Kartika",
    "Gilang",
    "Ratna",
    "Wahyu",
    "Sari",
    "Irfan",
    "Mega",
]
LAST = [
    "Santoso",
    "Wijaya",
    "Pratama",
    "Saputra",
    "Hidayat",
    "Kusuma",
    "Setiawan",
    "Nugroho",
    "Lestari",
    "Siregar",
    "Nasution",
    "Hutapea",
    "Wibowo",
    "Gunawan",
    "Rahmawati",
    "Susanto",
    "Halim",
    "Tanjung",
    "Purba",
    "Situmorang",
    "Utami",
    "Permana",
    "Firmansyah",
    "Syahputra",
]
CITIES = {  # city → (district list, IPv4 /16 prefix)
    "Jakarta": (["Menteng", "Kebayoran Baru", "Tebet", "Cengkareng", "Kelapa Gading"], "36.72"),
    "Bandung": (["Coblong", "Sukajadi", "Lengkong", "Antapani"], "36.80"),
    "Surabaya": (["Gubeng", "Wonokromo", "Rungkut", "Tegalsari"], "114.4"),
    "Medan": (["Medan Baru", "Medan Johor", "Medan Kota"], "180.241"),
    "Makassar": (["Panakkukang", "Tamalate", "Rappocini"], "125.160"),
    "Yogyakarta": (["Gondokusuman", "Umbulharjo", "Kotagede"], "182.253"),
    "Denpasar": (["Denpasar Selatan", "Denpasar Barat"], "103.28"),
    "Semarang": (["Tembalang", "Banyumanik", "Candisari"], "114.5"),
}
FOREIGN = {"US": "23.95", "NG": "105.112", "RU": "5.18", "VN": "113.161", "BR": "177.37", "RO": "86.120"}
STREETS = [
    "Melati",
    "Mawar",
    "Sudirman",
    "Thamrin",
    "Diponegoro",
    "Gatot Subroto",
    "Ahmad Yani",
    "Merdeka",
    "Pahlawan",
    "Kenanga",
    "Anggrek",
    "Cempaka",
    "Veteran",
    "Siliwangi",
    "Asia Afrika",
    "Pemuda",
]
BINS_ID = ["461700", "524261", "421570", "552338", "437527", "510510"]
BINS_FOREIGN = {
    "US": ["400022", "414720", "426684"],
    "GB": ["535316", "454313"],
    "BR": ["498401"],
    "RU": ["427601"],
}
UA = [
    "Android 14; SM-A546E",
    "Android 13; Redmi Note 12",
    "iOS 17.5; iPhone 13",
    "iOS 18.0; iPhone 15",
    "Windows NT 10.0; Chrome 128",
    "Macintosh; Safari 17.6",
    "Android 12; vivo 1935",
]
MCC = ["5411", "5311", "5732", "5651", "5812", "5999", "4814", "5945"]
DOMAINS = ["gmail.com", "yahoo.co.id", "outlook.com", "contoh.id"]


def luhn_complete(prefix: str, length: int, rng: random.Random) -> str:
    body = prefix + "".join(str(rng.randrange(10)) for _ in range(length - len(prefix) - 1))
    total = 0
    for i, ch in enumerate(reversed(body)):
        d = int(ch)
        if i % 2 == 0:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return body + str((10 - total % 10) % 10)


def pretty_pan(pan: str) -> str:
    return " ".join(pan[i : i + 4] for i in range(0, len(pan), 4))


@dataclass
class Card:
    pan: str
    issuer_country: str


@dataclass
class Customer:
    key: str  # internal stable key (never sent)
    name: str
    email: str
    phone: str
    registered: datetime
    kyc: int
    segment: str
    income: int
    city: str
    district: str
    address: str
    devices: list[str]
    cards: list[Card]
    bank_account: str
    ip_prefix: str
    rate: float  # expected sessions per day
    avg_amount: float
    ext_id: str = ""
    shared_address_variants: list[str] = field(default_factory=list)


@dataclass
class SimEvent:
    ts: datetime  # timezone-aware (WIB)
    record: dict[str, Any]
    customer_key: str
    fraud_type: str | None = None  # ground truth, never sent

    @property
    def is_fraud(self) -> bool:
        return self.fraud_type is not None


class World:
    """Factory for entities; every random choice goes through one seeded RNG → fully deterministic."""

    def __init__(self, rng: random.Random, start: datetime, end: datetime) -> None:
        self.rng = rng
        self.start = start
        self.end = end
        self._n = 0

    def _uid(self, prefix: str) -> str:
        self._n += 1
        return f"{prefix}-{self._n:06d}-{self.rng.randrange(16**4):04x}"

    def name(self) -> str:
        return f"{self.rng.choice(FIRST)} {self.rng.choice(LAST)}"

    def phone(self) -> str:
        return (
            "08"
            + self.rng.choice(["11", "12", "13", "21", "22", "52", "57", "77", "78", "95", "96"])
            + "".join(str(self.rng.randrange(10)) for _ in range(8))
        )

    def address(self, city: str, district: str) -> str:
        return f"Jl. {self.rng.choice(STREETS)} No. {self.rng.randint(1, 180)}, {district}, {city}"

    def address_variant(self, address: str) -> str:
        """Same place, written differently (what promo/refund farms do to dodge exact matching)."""
        v = address
        choice = self.rng.randrange(4)
        if choice == 0:
            v = v.replace("Jl. ", "Jalan ")
        elif choice == 1:
            v = v.replace("No. ", "No.")
        elif choice == 2:
            v = v.replace("No. ", "") + f" Blok {self.rng.choice('ABCD')}"
        else:
            v = v.lower()
        return v

    def phone_variant(self, phone: str) -> str:
        i = self.rng.randrange(len(phone) - 3, len(phone))
        return phone[:i] + str((int(phone[i]) + 1) % 10) + phone[i + 1 :]

    def device(self) -> str:
        return f"dev-{self.rng.randrange(16**10):010x}"

    def card(self, country: str = "ID") -> Card:
        bins = BINS_ID if country == "ID" else BINS_FOREIGN.get(country, BINS_FOREIGN["US"])
        return Card(luhn_complete(self.rng.choice(bins), 16, self.rng), country)

    def bank_account(self) -> str:
        return "".join(str(self.rng.randrange(10)) for _ in range(10))

    def ip(self, prefix: str) -> str:
        return f"{prefix}.{self.rng.randint(1, 254)}.{self.rng.randint(1, 254)}"

    def customer(self, *, registered: datetime | None = None, rate: float | None = None) -> Customer:
        rng = self.rng
        city = rng.choice(list(CITIES))
        district = rng.choice(CITIES[city][0])
        name = self.name()
        local = name.lower().replace(" ", rng.choice([".", "_", ""])) + str(rng.randint(1, 999))
        reg = registered or self.start - timedelta(days=rng.randint(30, 1500))
        segment = rng.choices(["regular", "premium", "new"], weights=[75, 15, 10])[0]
        income = int(rng.lognormvariate(15.9, 0.5))  # ~ Rp 8 jt median
        return Customer(
            key=self._uid("C"),
            name=name,
            email=f"{local}@{rng.choice(DOMAINS)}",
            phone=self.phone(),
            registered=reg,
            kyc=rng.choices([1, 2, 3], weights=[20, 60, 20])[0],
            segment=segment,
            income=income,
            city=city,
            district=district,
            address=self.address(city, district),
            devices=[self.device() for _ in range(rng.choices([1, 2], weights=[80, 20])[0])],
            cards=[self.card() for _ in range(rng.choices([1, 2], weights=[75, 25])[0])],
            bank_account=self.bank_account(),
            ip_prefix=CITIES[city][1],
            rate=rate if rate is not None else rng.lognormvariate(-1.3, 0.8),
            avg_amount=rng.lognormvariate(12.2, 0.7) * (2.5 if segment == "premium" else 1.0),
        )

    # ---------------------------------------------------------------- record builder
    def record(
        self,
        kind: str,
        ts: datetime,
        c: Customer,
        *,
        amount: float | None = None,
        device: str | None = None,
        ip: str | None = None,
        country: str = "ID",
        city: str | None = None,
        card: Card | None = None,
        method: str | None = None,
        recipient: str | None = None,
        promo: str | None = None,
        discount: float = 0.0,
        cashback: float = 0.0,
        shipping: str | None = None,
        change: str | None = None,
        login_ok: bool | None = None,
        api_client: str | None = None,
        ref: str | None = None,
        channel: str | None = None,
        merchant: str | None = None,
        status: str = "BERHASIL",
    ) -> dict[str, Any]:
        rng = self.rng
        pay_kinds = ("transaction", "promo_redemption", "payout", "refund", "transfer")
        method = method or (
            ("card" if card else rng.choice(["ewallet", "va", "bank_transfer"]))
            if kind in pay_kinds
            else None
        )
        if method == "card" and card is None:
            card = c.cards[0]
        ship = shipping or (c.address if kind in ("transaction", "promo_redemption") else None)
        return {
            "no_ref": None,  # assigned after chronological sort (deterministic numbering)
            "waktu": ts.astimezone(WIB).strftime(DATETIME_FORMAT),
            "jenis": JENIS[kind],
            "status": status,
            "pengguna": {
                "id": c.ext_id,
                "nama": c.name,
                "email": c.email,
                "no_hp": c.phone,
                "tgl_daftar": c.registered.astimezone(WIB).strftime("%Y-%m-%d"),
                "kyc": c.kyc,
                "segmen": c.segment,
                "penghasilan": c.income,
            },
            "nominal": fmt_amount(round(amount, 2)) if amount is not None else None,
            "mata_uang": "IDR",
            "kanal": channel or rng.choices(["mobile_app", "web"], weights=[70, 30])[0],
            "merchant": (
                {"id": merchant or f"M-{rng.randint(1, 400):04d}", "kategori": rng.choice(MCC)}
                if kind in ("transaction", "promo_redemption", "refund")
                else None
            ),
            "pembayaran": (
                {
                    "metode": method,
                    "no_kartu": pretty_pan(card.pan) if card else None,
                    "negara_penerbit": card.issuer_country if card else None,
                    "rekening_tujuan": recipient,
                }
                if method
                else None
            ),
            "perangkat": {
                "id": device or rng.choice(c.devices),
                "ip": ip or self.ip(c.ip_prefix),
                "negara": country,
                "kota": city or (c.city if country == "ID" else None),
                "ua": rng.choice(UA),
            },
            "voucher": {"kode": promo, "diskon": round(discount, 2), "cashback": round(cashback, 2)},
            "pengiriman": {"alamat": ship},
            "penagihan": {"alamat": c.address if kind == "transaction" else None},
            "perubahan_akun": change,
            "login_berhasil": login_ok,
            "id_klien_api": api_client,
            "ref_transaksi": ref,
        }

    def rand_ts(
        self, lo: datetime | None = None, hi: datetime | None = None, *, night: bool = False
    ) -> datetime:
        lo, hi = lo or self.start, hi or self.end
        span = max(1.0, (hi - lo).total_seconds())
        ts = lo + timedelta(seconds=self.rng.uniform(0, span))
        if night:
            local = ts.astimezone(WIB)
            ts = local.replace(hour=self.rng.randint(0, 4), minute=self.rng.randint(0, 59)).astimezone(UTC)
            ts = min(max(ts, lo), hi)
        return ts

    def daytime(self, day: datetime) -> datetime:
        """A plausible local activity time on the given day (peaks at lunch and evening)."""
        hour = int(
            self.rng.choices(
                range(24),
                weights=[1, 1, 1, 1, 1, 2, 4, 6, 7, 7, 7, 8, 10, 9, 7, 7, 7, 8, 10, 12, 12, 10, 6, 3],
            )[0]
        )
        local = day.astimezone(WIB).replace(
            hour=hour, minute=self.rng.randint(0, 59), second=self.rng.randint(0, 59), microsecond=0
        )
        return local.astimezone(UTC)
