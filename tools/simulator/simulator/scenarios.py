"""Per-project behaviour generators: normal activity + injected fraud typologies with ground truth."""

from __future__ import annotations

import math
import random
from collections import Counter
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Any

from simulator.world import FOREIGN, Customer, SimEvent, World


@dataclass(frozen=True)
class ProjectSpec:
    slug: str
    name: str
    stage: str
    prefix: str
    customer_share: float
    description: str
    business_context: str


PROJECTS: list[ProjectSpec] = [
    ProjectSpec(
        "checkout",
        "Checkout Protection",
        "pre_payment",
        "CHK",
        1.0,
        "Pre-payment scoring for marketplace checkout",
        "Scores logins, account changes and checkout payments before authorisation. Threats: carding, "
        "account takeover, compromised partner API clients.",
    ),
    ProjectSpec(
        "post-payment",
        "Post-payment & Payout",
        "post_payment",
        "PST",
        0.5,
        "After payment: transfers, wallet payouts",
        "Scores transfers and wallet payouts after payment. Threats: bank/e-wallet account takeover, "
        "money mules.",
    ),
    ProjectSpec(
        "returns",
        "Returns & Refunds",
        "returns",
        "RTN",
        0.35,
        "Return / refund requests",
        "Scores return and refund requests. Threat: serial refund abuse by linked accounts.",
    ),
    ProjectSpec(
        "promo",
        "Promo & Cashback",
        "promo",
        "PRM",
        0.5,
        "Voucher, discount and cashback redemptions",
        "Scores voucher redemptions and cashback payouts; every transaction may be technically legit. "
        "Threat: promo farms of multi-accounts.",
    ),
]


@dataclass
class Dataset:
    project: ProjectSpec
    events: list[SimEvent]
    customers: list[Customer]
    fraud_customers: dict[str, str] = field(default_factory=dict)  # customer_key → fraud_type

    def typology_counts(self) -> Counter[str]:
        return Counter(e.fraud_type for e in self.events if e.fraud_type)


class ProjectSimulator:
    def __init__(
        self, spec: ProjectSpec, n_customers: int, days: int, seed: int, end: datetime, run_tag: str = ""
    ) -> None:
        self.spec = spec
        # A run tag namespaces external ids so a second run (e.g. an out-of-sample evaluation) creates new
        # customers/events instead of being de-duplicated against an earlier run.
        self.id_prefix = f"{spec.prefix}-{run_tag}" if run_tag else spec.prefix
        self.rng = random.Random(f"{seed}:{spec.slug}")
        self.end = end
        self.start = end - timedelta(days=days)
        self.days = days
        self.n = max(20, int(n_customers * spec.customer_share))
        self.w = World(self.rng, self.start, self.end)
        self.events: list[SimEvent] = []
        self.customers: list[Customer] = []
        self.fraud_customers: dict[str, str] = {}

    # ------------------------------------------------------------------ helpers
    def emit(
        self, kind: str, ts: datetime, c: Customer, fraud: str | None = None, **kw: Any
    ) -> dict[str, Any]:
        rec = self.w.record(kind, ts, c, **kw)
        self.events.append(SimEvent(ts, rec, c.key, fraud))
        return rec

    def amount(self, c: Customer, factor: float = 1.0) -> float:
        return round(max(5000.0, self.rng.lognormvariate(math.log(c.avg_amount), 0.6) * factor), -2)

    def new_customer(self, **kw: Any) -> Customer:
        c = self.w.customer(**kw)
        self.customers.append(c)
        return c

    def active_days(self, c: Customer) -> list[datetime]:
        out = []
        first = max(self.start, c.registered)
        d = first
        while d < self.end:
            # Poisson number of sessions that day
            k = self._poisson(c.rate)
            out.extend([d] * k)
            d += timedelta(days=1)
        return out

    def _poisson(self, lam: float) -> int:
        limit, k, p = math.exp(-lam), 0, 1.0
        while True:
            p *= self.rng.random()
            if p <= limit:
                return k
            k += 1

    # ------------------------------------------------------------------ normal behaviour
    def normal_population(self) -> list[Customer]:
        pop = []
        for _ in range(self.n):
            new = self.rng.random() < 0.08
            reg = self.w.rand_ts(self.start, self.end - timedelta(days=3)) if new else None
            c = self.new_customer(registered=reg)
            pop.append(c)
            if new and self.spec.stage in ("pre_payment", "promo"):
                self.emit("registration", c.registered, c, device=c.devices[0])
        return pop

    def normal_checkout(self, pop: list[Customer]) -> None:
        for c in pop:
            for day in self.active_days(c):
                ts = self.w.daytime(day)
                if ts < c.registered or ts > self.end:
                    continue
                dev = self.rng.choice(c.devices)
                if self.rng.random() < 0.03:
                    self.emit("login", ts - timedelta(seconds=40), c, device=dev, login_ok=False)
                self.emit("login", ts, c, device=dev, login_ok=True)
                for i in range(self.rng.choices([0, 1, 2], weights=[35, 55, 10])[0]):
                    t = ts + timedelta(minutes=self.rng.randint(2, 25) * (i + 1))
                    card = self.rng.choice(c.cards) if self.rng.random() < 0.6 else None
                    self.emit("transaction", t, c, amount=self.amount(c), device=dev, card=card)
                if self.rng.random() < 0.01:
                    self.emit(
                        "account_change",
                        ts + timedelta(minutes=1),
                        c,
                        device=dev,
                        change=self.rng.choice(["email", "phone", "address", "2fa"]),
                    )

    def normal_post_payment(self, pop: list[Customer]) -> None:
        for c in pop:
            for day in self.active_days(c):
                ts = self.w.daytime(day)
                if self.rng.random() < 0.75:
                    other = self.rng.choice(pop)
                    self.emit(
                        "transfer",
                        ts,
                        c,
                        amount=self.amount(c, 0.8),
                        method="bank_transfer",
                        recipient=other.bank_account,
                    )
                else:
                    self.emit(
                        "payout",
                        ts,
                        c,
                        amount=self.amount(c, 1.5),
                        method="bank_transfer",
                        recipient=c.bank_account,
                    )

    def normal_returns(self, pop: list[Customer]) -> None:
        for c in pop:
            for day in self.active_days(c):
                ts = self.w.daytime(day)
                rec = self.emit("transaction", ts, c, amount=self.amount(c))
                if self.rng.random() < 0.04:
                    rts = ts + timedelta(days=self.rng.uniform(3, 14))
                    if rts < self.end:
                        self._refund(c, rts, rec, reason_fraud=None)

    def _refund(
        self, c: Customer, ts: datetime, original: dict[str, Any], reason_fraud: str | None, **kw: Any
    ) -> None:
        amt = original["nominal"]
        rec = self.emit("refund", ts, c, reason_fraud, amount=None, **kw)
        rec["nominal"] = amt
        rec["ref_transaksi"] = original  # patched to the original no_ref after numbering
        rec["alasan_retur"] = (
            self.rng.choice(["barang tidak sampai", "barang rusak", "tidak sesuai"])
            if reason_fraud
            else self.rng.choice(["ukuran tidak cocok", "berubah pikiran", "barang rusak"])
        )

    def normal_promo(self, pop: list[Customer]) -> None:
        codes = ["HEMAT10", "GAJIAN20", "ONGKIRFREE", "FLASH15"]
        for c in pop:
            for day in self.active_days(c):
                ts = self.w.daytime(day)
                amt = self.amount(c)
                if self.rng.random() < 0.2:
                    disc = round(min(amt * 0.1, 50_000), -2)
                    self.emit(
                        "promo_redemption",
                        ts,
                        c,
                        amount=amt - disc,
                        promo=self.rng.choice(codes),
                        discount=disc,
                        cashback=0.0,
                    )
                else:
                    self.emit("transaction", ts, c, amount=amt)

    # ------------------------------------------------------------------ typologies
    def carding_rings(self) -> None:
        rings = max(2, self.n // 400)
        for _ in range(rings):
            devices = [self.w.device() for _ in range(self.rng.randint(1, 2))]
            country = self.rng.choice(list(FOREIGN))
            stolen = [
                self.w.card(self.rng.choice(["US", "GB", "BR", "ID"])) for _ in range(self.rng.randint(6, 15))
            ]
            accounts = [
                self.new_customer(registered=self.w.rand_ts(self.start, self.end - timedelta(days=5)), rate=0)
                for _ in range(self.rng.randint(3, 6))
            ]
            for acc in accounts:
                acc.devices = devices
                self.fraud_customers[acc.key] = "carding"
                self.emit("registration", acc.registered, acc, "carding", device=devices[0])
            for _ in range(self.rng.randint(2, 4)):  # attack waves
                t0 = self.w.rand_ts(max(a.registered for a in accounts), self.end - timedelta(hours=2))
                ip_country = self.rng.choice([country, "ID"])
                ip_prefix = FOREIGN.get(ip_country, "36.72")
                t = t0
                for card in self.rng.sample(stolen, k=min(len(stolen), self.rng.randint(4, 9))):
                    acc = self.rng.choice(accounts)
                    t += timedelta(seconds=self.rng.randint(20, 180))
                    self.emit(
                        "transaction",
                        t,
                        acc,
                        "carding",
                        amount=float(self.rng.randint(1, 6) * 10_000),
                        card=card,
                        device=self.rng.choice(devices),
                        ip=self.w.ip(ip_prefix),
                        country=ip_country,
                        status=self.rng.choice(["BERHASIL", "GAGAL"]),
                    )
                for _ in range(self.rng.randint(1, 3)):
                    acc = self.rng.choice(accounts)
                    t += timedelta(minutes=self.rng.randint(2, 20))
                    self.emit(
                        "transaction",
                        t,
                        acc,
                        "carding",
                        amount=float(self.rng.randint(30, 150) * 100_000),
                        card=self.rng.choice(stolen),
                        device=self.rng.choice(devices),
                        ip=self.w.ip(ip_prefix),
                        country=ip_country,
                        shipping=self.w.address("Jakarta", "Cengkareng"),
                    )

    def account_takeovers(self, pop: list[Customer]) -> None:
        victims = self.rng.sample(pop, k=max(2, len(pop) // 100))
        for v in victims:
            t = self.w.rand_ts(self.start + timedelta(days=2), self.end - timedelta(hours=3))
            atk_dev = self.w.device()
            atk_country = self.rng.choice(["ID", "ID", *FOREIGN])
            atk_ip = self.w.ip(FOREIGN.get(atk_country, "180.241"))
            for _ in range(self.rng.randint(3, 8)):
                t += timedelta(seconds=self.rng.randint(5, 60))
                self.emit(
                    "login",
                    t,
                    v,
                    "account_takeover",
                    device=atk_dev,
                    ip=atk_ip,
                    country=atk_country,
                    login_ok=False,
                )
            t += timedelta(seconds=30)
            self.emit(
                "login",
                t,
                v,
                "account_takeover",
                device=atk_dev,
                ip=atk_ip,
                country=atk_country,
                login_ok=True,
            )
            new_addr = self.w.address(self.rng.choice(["Jakarta", "Medan", "Makassar"]), "Cengkareng")
            for change in ("password", "phone", "address"):
                t += timedelta(minutes=self.rng.randint(1, 5))
                self.emit(
                    "account_change",
                    t,
                    v,
                    "account_takeover",
                    device=atk_dev,
                    ip=atk_ip,
                    country=atk_country,
                    change=change,
                )
            for _ in range(self.rng.randint(1, 3)):
                t += timedelta(minutes=self.rng.randint(3, 40))
                self.emit(
                    "transaction",
                    t,
                    v,
                    "account_takeover",
                    amount=float(self.rng.randint(50, 250) * 100_000),
                    device=atk_dev,
                    ip=atk_ip,
                    country=atk_country,
                    card=v.cards[0],
                    shipping=new_addr,
                )

    def system_breach(self, pop: list[Customer]) -> None:
        incidents = max(1, self.days // 30)
        for k in range(incidents):
            t = self.w.rand_ts(self.start + timedelta(days=3), self.end - timedelta(hours=1), night=True)
            client = f"partner-{self.rng.randint(1, 9)}"
            ips = [self.w.ip(self.rng.choice(list(FOREIGN.values()))) for _ in range(3)]
            base = float(self.rng.randint(20, 50) * 100_000)
            burst = max(20, min(300, self.n // 8))
            for _ in range(self.rng.randint(burst // 2, burst)):
                v = self.rng.choice(pop)
                t += timedelta(seconds=self.rng.uniform(2, 8))
                self.emit(
                    "transaction",
                    t,
                    v,
                    "system_breach",
                    amount=base + self.rng.randint(-5, 5) * 1000,
                    channel="api",
                    api_client=client,
                    ip=self.rng.choice(ips),
                    device=f"api-{client}",
                    method="bank_transfer",
                    recipient=self.w.bank_account() if k % 2 else None,
                )

    def bank_account_takeovers(self, pop: list[Customer]) -> None:
        victims = self.rng.sample(pop, k=max(2, len(pop) // 100))
        for v in victims:
            t = self.w.rand_ts(self.start + timedelta(days=2), self.end - timedelta(hours=3), night=True)
            dev, ip = self.w.device(), self.w.ip(self.rng.choice(["180.241", "125.160", "23.95"]))
            self.emit("login", t, v, "bank_account_takeover", device=dev, ip=ip, login_ok=True)
            atk_acct = self.w.bank_account()
            t += timedelta(minutes=2)
            self.emit(
                "account_change", t, v, "bank_account_takeover", device=dev, ip=ip, change="beneficiary"
            )
            for _ in range(self.rng.randint(2, 4)):
                t += timedelta(minutes=self.rng.randint(1, 15))
                self.emit(
                    "payout",
                    t,
                    v,
                    "bank_account_takeover",
                    amount=float(self.rng.randint(50, 500) * 100_000),
                    device=dev,
                    ip=ip,
                    method="bank_transfer",
                    recipient=atk_acct,
                )

    def money_mules(self, pop: list[Customer]) -> None:
        for _ in range(max(2, self.n // 300)):
            mule = self.new_customer(
                registered=self.w.rand_ts(self.start, self.end - timedelta(days=10)), rate=0
            )
            self.fraud_customers[mule.key] = "money_mule"
            t = self.w.rand_ts(mule.registered + timedelta(days=1), self.end - timedelta(days=4))
            for _ in range(self.rng.randint(8, 25)):  # fan-in from scam victims
                victim = self.rng.choice(pop)
                t += timedelta(minutes=self.rng.randint(5, 240))
                self.emit(
                    "transfer",
                    t,
                    victim,
                    "money_mule",
                    amount=float(self.rng.randint(5, 80) * 100_000),
                    method="bank_transfer",
                    recipient=mule.bank_account,
                )
            cashout = self.w.bank_account()
            for _ in range(self.rng.randint(2, 5)):  # fast fan-out
                t += timedelta(minutes=self.rng.randint(10, 90))
                self.emit(
                    "payout",
                    t,
                    mule,
                    "money_mule",
                    amount=float(self.rng.randint(50, 300) * 100_000),
                    method="bank_transfer",
                    recipient=cashout,
                )

    def refund_abusers(self) -> None:
        for _ in range(max(2, self.n // 150)):
            base = self.new_customer(rate=0)
            ring = [base] + [self.new_customer(rate=0) for _ in range(self.rng.randint(0, 3))]
            dev = base.devices[0]
            for acc in ring:
                self.fraud_customers[acc.key] = "refund_abuse"
                acc.devices = [dev]
                acc.address = self.w.address_variant(base.address)
            for acc in ring:
                for _ in range(self.rng.randint(4, 10)):
                    ts = self.w.rand_ts(self.start, self.end - timedelta(days=3))
                    rec = self.emit("transaction", ts, acc, amount=self.amount(acc, 1.5), device=dev)
                    if self.rng.random() < 0.75:
                        self._refund(
                            acc,
                            ts + timedelta(hours=self.rng.uniform(6, 48)),
                            rec,
                            "refund_abuse",
                            device=dev,
                        )

    def promo_farms(self) -> None:
        for _ in range(max(2, self.n // 250)):
            organiser = self.w.customer()
            devices = [self.w.device() for _ in range(self.rng.randint(1, 3))]
            code = self.rng.choice(["NEWUSER50", "CASHBACK100", "WELCOME30"])
            t0 = self.w.rand_ts(self.start + timedelta(days=5), self.end - timedelta(days=2))
            for _ in range(self.rng.randint(5, 20)):
                acc = self.new_customer(
                    registered=t0 + timedelta(minutes=self.rng.randint(0, 3 * 24 * 60)), rate=0
                )
                self.fraud_customers[acc.key] = "promo_abuse"
                acc.devices = [self.rng.choice(devices)]
                acc.address = self.w.address_variant(organiser.address)
                acc.phone = self.w.phone_variant(organiser.phone)
                acc.segment = "new"
                if acc.registered >= self.end:
                    continue
                self.emit("registration", acc.registered, acc, "promo_abuse", device=acc.devices[0])
                t = acc.registered + timedelta(minutes=self.rng.randint(5, 180))
                amt = float(self.rng.randint(10, 30) * 10_000)
                disc = round(amt * 0.5, -2)
                if t < self.end:
                    self.emit(
                        "promo_redemption",
                        t,
                        acc,
                        "promo_abuse",
                        amount=amt - disc,
                        promo=code,
                        discount=disc,
                        cashback=round(amt * 0.1, -2) if "CASHBACK" in code else 0.0,
                        device=acc.devices[0],
                    )
                if self.rng.random() < 0.5 and t + timedelta(days=1) < self.end:
                    self.emit(
                        "payout",
                        t + timedelta(days=1),
                        acc,
                        "promo_abuse",
                        amount=float(self.rng.randint(2, 10) * 10_000),
                        method="ewallet",
                        recipient=organiser.bank_account,
                        device=acc.devices[0],
                    )

    # ------------------------------------------------------------------ assembly
    def generate(self) -> Dataset:
        pop = self.normal_population()
        stage = self.spec.stage
        if stage == "pre_payment":
            self.normal_checkout(pop)
            self.carding_rings()
            self.account_takeovers(pop)
            self.system_breach(pop)
        elif stage == "post_payment":
            self.normal_post_payment(pop)
            self.bank_account_takeovers(pop)
            self.money_mules(pop)
        elif stage == "returns":
            self.normal_returns(pop)
            self.refund_abusers()
        elif stage == "promo":
            self.normal_promo(pop)
            self.promo_farms()
        return self._finalise()

    def _finalise(self) -> Dataset:
        # External ids must not leak ground truth: shuffle customers before numbering.
        order = list(self.customers)
        self.rng.shuffle(order)
        for i, c in enumerate(order, 1):
            c.ext_id = f"{self.id_prefix}-U{i:06d}"
        by_key = {c.key: c for c in self.customers}
        self.events = [e for e in self.events if self.start <= e.ts <= self.end]
        self.events.sort(key=lambda e: (e.ts, e.customer_key))
        for i, e in enumerate(self.events, 1):
            e.record["no_ref"] = f"{self.id_prefix}-{i:08d}"
            e.record["pengguna"]["id"] = by_key[e.customer_key].ext_id
        for e in self.events:  # resolve refund → original order reference (original has a lower number)
            ref = e.record.get("ref_transaksi")
            if isinstance(ref, dict):
                e.record["ref_transaksi"] = ref["no_ref"]
        return Dataset(self.spec, self.events, self.customers, self.fraud_customers)


def generate_all(
    n_customers: int,
    days: int,
    seed: int,
    end: datetime,
    only: list[str] | None = None,
    run_tag: str = "",
) -> list[Dataset]:
    return [
        ProjectSimulator(p, n_customers, days, seed, end, run_tag).generate()
        for p in PROJECTS
        if not only or p.slug in only
    ]
