"""SQL access for the ml schema and the read-only core tables ml-service may use.

Every function takes a `Connection` opened with `tenant_session()` and filters by `project_id` explicitly.
"""

from __future__ import annotations

import json
from collections.abc import Iterable
from datetime import datetime
from typing import Any
from uuid import UUID

from sqlalchemy import text
from sqlalchemy.engine import Connection, RowMapping

MODEL_COLUMNS = """
    id, tenant_id, project_id, kind, version, algorithms, params, feature_set_version, feature_names, metrics,
    training_history, artifact_path, status, progress, trained_rows, error, submitted_by, created_by,
    training_started_at, training_finished_at, activated_at
"""


def _json(value: Any) -> str:
    return json.dumps(value, default=str)


def model_to_dict(row: RowMapping) -> dict[str, Any]:
    out = dict(row)
    for key in ("id", "tenant_id", "project_id", "submitted_by", "created_by"):
        if out.get(key) is not None:
            out[key] = str(out[key])
    return out


# ------------------------------------------------------------------------------------------ projects
def get_project(conn: Connection, project_id: UUID) -> RowMapping | None:
    return (
        conn.execute(
            text("SELECT id, tenant_id, status, ml_config FROM core.projects WHERE id = :p"),
            {"p": str(project_id)},
        )
        .mappings()
        .first()
    )


# ------------------------------------------------------------------------------------------ models
def create_model(
    conn: Connection,
    *,
    tenant_id: UUID,
    project_id: UUID,
    kind: str,
    algorithms: dict[str, Any],
    params: dict[str, Any],
    feature_set_version: int,
    created_by: UUID | None,
) -> RowMapping:
    # version = max+1 under a transaction-scoped advisory lock (per project & kind) to avoid races
    conn.execute(text("SELECT pg_advisory_xact_lock(hashtext(:k))"), {"k": f"ml.models:{project_id}:{kind}"})
    version = conn.execute(
        text("SELECT COALESCE(MAX(version), 0) + 1 FROM ml.models WHERE project_id = :p AND kind = :k"),
        {"p": str(project_id), "k": kind},
    ).scalar_one()
    row = (
        conn.execute(
            text(
                f"""
            INSERT INTO ml.models (tenant_id, project_id, kind, version, algorithms, params, feature_set_version,
                                   status, created_by)
            VALUES (:t, :p, :k, :v, CAST(:alg AS jsonb), CAST(:params AS jsonb), :fsv, 'training', :cb)
            RETURNING {MODEL_COLUMNS}
            """
            ),
            {
                "t": str(tenant_id),
                "p": str(project_id),
                "k": kind,
                "v": version,
                "alg": _json(algorithms),
                "params": _json(params),
                "fsv": feature_set_version,
                "cb": str(created_by) if created_by else None,
            },
        )
        .mappings()
        .one()
    )
    return row


_JSON_FIELDS = {"metrics", "training_history", "feature_names", "algorithms", "params"}


def update_model(conn: Connection, model_id: UUID, **fields: Any) -> None:
    if not fields:
        return
    sets = []
    params: dict[str, Any] = {"id": str(model_id)}
    for key, value in fields.items():
        if key in _JSON_FIELDS:
            sets.append(f"{key} = CAST(:{key} AS jsonb)")
            params[key] = _json(value)
        elif key.endswith("_at") and value == "now":
            sets.append(f"{key} = now()")
        else:
            sets.append(f"{key} = :{key}")
            params[key] = str(value) if isinstance(value, UUID) else value
    conn.execute(text(f"UPDATE ml.models SET {', '.join(sets)} WHERE id = :id"), params)


def get_model(conn: Connection, project_id: UUID, model_id: UUID) -> RowMapping | None:
    return (
        conn.execute(
            text(f"SELECT {MODEL_COLUMNS} FROM ml.models WHERE project_id = :p AND id = :id"),
            {"p": str(project_id), "id": str(model_id)},
        )
        .mappings()
        .first()
    )


def get_active_model(conn: Connection, project_id: UUID, kind: str) -> RowMapping | None:
    return (
        conn.execute(
            text(
                f"SELECT {MODEL_COLUMNS} FROM ml.models WHERE project_id = :p AND kind = :k AND status = 'active'"
            ),
            {"p": str(project_id), "k": kind},
        )
        .mappings()
        .first()
    )


def list_models(
    conn: Connection, project_id: UUID, kind: str | None, status: str | None, page: int, page_size: int
) -> tuple[list[RowMapping], int]:
    where = ["project_id = :p"]
    params: dict[str, Any] = {"p": str(project_id), "lim": page_size, "off": (page - 1) * page_size}
    if kind:
        where.append("kind = :k")
        params["k"] = kind
    if status:
        where.append("status = :s")
        params["s"] = status
    clause = " AND ".join(where)
    total = conn.execute(text(f"SELECT count(*) FROM ml.models WHERE {clause}"), params).scalar_one()
    rows = (
        conn.execute(
            text(
                f"SELECT {MODEL_COLUMNS} FROM ml.models WHERE {clause} "
                "ORDER BY training_started_at DESC LIMIT :lim OFFSET :off"
            ),
            params,
        )
        .mappings()
        .all()
    )
    return list(rows), int(total)


def mark_interrupted(
    conn: Connection, project_id: UUID, running: set[str], process_started_at: datetime
) -> None:
    """Models stuck in 'training' from before this process started were interrupted by a restart."""
    conn.execute(
        text(
            """
            UPDATE ml.models SET status = 'failed', error = 'training interrupted by service restart',
                   training_finished_at = now()
            WHERE project_id = :p AND status = 'training' AND training_started_at < :started
              AND NOT (id::text = ANY(:running))
            """
        ),
        {"p": str(project_id), "started": process_started_at, "running": list(running)},
    )


def archive_active(conn: Connection, project_id: UUID, kind: str) -> None:
    conn.execute(
        text(
            "UPDATE ml.models SET status = 'archived' WHERE project_id = :p AND kind = :k AND status = 'active'"
        ),
        {"p": str(project_id), "k": kind},
    )


# ------------------------------------------------------------------------------------------ training data
def load_training_rows(
    conn: Connection,
    project_id: UUID,
    *,
    since_days: int | None,
    limit: int,
    labelled_only: bool,
    include_payload: bool,
    mature_unlabelled_days: int | None = None,
) -> list[RowMapping]:
    """Events + features (+ latest label), oldest → newest (time-ordered for time-based splits).

    With ``labelled_only`` and ``mature_unlabelled_days``, unlabelled events older than that many days are
    included too (their ``label`` is NULL; callers treat them as legit — label maturity).
    """
    mature = ""
    if labelled_only and mature_unlabelled_days:
        join = "LEFT JOIN"
        mature = "AND (l.label IS NOT NULL OR e.occurred_at < now() - make_interval(days => :mature_days))"
    else:
        join = "JOIN" if labelled_only else "LEFT JOIN"
    since = "AND e.occurred_at >= now() - make_interval(days => :days)" if since_days else ""
    payload = "e.payload" if include_payload else "NULL::jsonb AS payload"
    sql = f"""
        SELECT * FROM (
            SELECT e.id AS event_id, e.occurred_at, e.customer_id, f.features, {payload}, l.label
            FROM core.events e
            JOIN core.event_features f ON f.event_id = e.id
            {join} core.event_labels l ON l.event_id = e.id
            WHERE e.project_id = :p {since} {mature}
            ORDER BY e.occurred_at DESC
            LIMIT :lim
        ) recent ORDER BY occurred_at ASC
    """
    params: dict[str, Any] = {"p": str(project_id), "lim": limit}
    if since_days:
        params["days"] = since_days
    if mature:
        params["mature_days"] = mature_unlabelled_days
    return list(conn.execute(text(sql), params).mappings().all())


# ------------------------------------------------------------------------------------------ unsupervised
def replace_event_anomaly(
    conn: Connection, tenant_id: UUID, model_id: UUID, rows: list[dict[str, Any]]
) -> None:
    conn.execute(text("DELETE FROM ml.event_anomaly WHERE model_id = :m"), {"m": str(model_id)})
    if not rows:
        return
    payload = [{**r, "m": str(model_id), "t": str(tenant_id)} for r in rows]
    for start in range(0, len(payload), 5000):
        conn.execute(
            text(
                "INSERT INTO ml.event_anomaly (model_id, event_id, tenant_id, anomaly_score, cluster_id, pca_x, pca_y) "
                "VALUES (:m, :event_id, :t, :anomaly_score, :cluster_id, :pca_x, :pca_y)"
            ),
            payload[start : start + 5000],
        )


def replace_clusters(
    conn: Connection, tenant_id: UUID, model_id: UUID, clusters: Iterable[dict[str, Any]]
) -> None:
    conn.execute(text("DELETE FROM ml.clusters WHERE model_id = :m"), {"m": str(model_id)})
    for c in clusters:
        conn.execute(
            text(
                """
                INSERT INTO ml.clusters (model_id, tenant_id, cluster_id, size, fraud_rate, labeled_count, centroid,
                                         profile, top_features)
                VALUES (:m, :t, :cid, :size, :fr, :lc, CAST(:centroid AS jsonb), CAST(:profile AS jsonb),
                        CAST(:top AS jsonb))
                """
            ),
            {
                "m": str(model_id),
                "t": str(tenant_id),
                "cid": c["cluster_id"],
                "size": c["size"],
                "fr": c["fraud_rate"],
                "lc": c["labeled_count"],
                "centroid": _json(c["centroid"]),
                "profile": _json(c["profile"]),
                "top": _json(c["top_features"]),
            },
        )


def list_clusters(conn: Connection, model_id: UUID) -> list[RowMapping]:
    return list(
        conn.execute(
            text(
                "SELECT model_id, cluster_id, size, fraud_rate, labeled_count, profile, top_features, label, notes "
                "FROM ml.clusters WHERE model_id = :m ORDER BY (cluster_id = -1), size DESC"
            ),
            {"m": str(model_id)},
        )
        .mappings()
        .all()
    )


def cluster_fraud_rates(conn: Connection, model_id: UUID) -> dict[int, float | None]:
    rows = conn.execute(
        text("SELECT cluster_id, fraud_rate FROM ml.clusters WHERE model_id = :m"), {"m": str(model_id)}
    ).all()
    return {int(r[0]): (float(r[1]) if r[1] is not None else None) for r in rows}


def update_cluster_label(
    conn: Connection, model_id: UUID, cluster_id: int, label: str | None, notes: str | None
) -> RowMapping | None:
    return (
        conn.execute(
            text(
                "UPDATE ml.clusters SET label = :label, notes = :notes WHERE model_id = :m AND cluster_id = :c "
                "RETURNING model_id, cluster_id, size, fraud_rate, labeled_count, profile, top_features, label, notes"
            ),
            {"m": str(model_id), "c": cluster_id, "label": label, "notes": notes},
        )
        .mappings()
        .first()
    )


def projection(conn: Connection, model_id: UUID, limit: int) -> list[RowMapping]:
    return list(
        conn.execute(
            text(
                """
                SELECT a.event_id, a.pca_x AS x, a.pca_y AS y, a.cluster_id, a.anomaly_score, l.label
                FROM ml.event_anomaly a
                LEFT JOIN core.event_labels l ON l.event_id = a.event_id
                WHERE a.model_id = :m
                ORDER BY a.event_id           -- uuid order ≈ random sample
                LIMIT :lim
                """
            ),
            {"m": str(model_id), "lim": limit},
        )
        .mappings()
        .all()
    )


def top_anomalies(
    conn: Connection, project_id: UUID, model_id: UUID, limit: int, min_score: float
) -> list[RowMapping]:
    return list(
        conn.execute(
            text(
                """
                SELECT a.event_id, a.anomaly_score, a.cluster_id, e.external_id, e.event_type, e.occurred_at,
                       e.amount, e.customer_id, l.label
                FROM ml.event_anomaly a
                JOIN core.events e ON e.id = a.event_id AND e.project_id = :p
                LEFT JOIN core.event_labels l ON l.event_id = a.event_id
                WHERE a.model_id = :m AND a.anomaly_score >= :min
                ORDER BY a.anomaly_score DESC
                LIMIT :lim
                """
            ),
            {"p": str(project_id), "m": str(model_id), "lim": limit, "min": min_score},
        )
        .mappings()
        .all()
    )


# ------------------------------------------------------------------------------------------ graph communities
def fraud_customers(conn: Connection, project_id: UUID) -> set[str]:
    rows = conn.execute(
        text("SELECT id FROM core.customers WHERE project_id = :p AND risk_label = 'fraud'"),
        {"p": str(project_id)},
    ).all()
    return {str(r[0]) for r in rows}


def replace_communities(
    conn: Connection,
    tenant_id: UUID,
    project_id: UUID,
    assignments: dict[str, int],
    stats: list[dict[str, Any]],
) -> None:
    p, t = str(project_id), str(tenant_id)
    conn.execute(text("DELETE FROM ml.graph_communities WHERE project_id = :p"), {"p": p})
    conn.execute(text("DELETE FROM ml.graph_community_stats WHERE project_id = :p"), {"p": p})
    rows = [{"c": cid, "comm": comm, "t": t, "p": p} for cid, comm in assignments.items()]
    for start in range(0, len(rows), 5000):
        conn.execute(
            text(
                "INSERT INTO ml.graph_communities (customer_id, tenant_id, project_id, community_id) "
                "VALUES (:c, :t, :p, :comm) ON CONFLICT (customer_id) DO UPDATE SET "
                "community_id = EXCLUDED.community_id, project_id = EXCLUDED.project_id, computed_at = now()"
            ),
            rows[start : start + 5000],
        )
    if stats:
        conn.execute(
            text(
                "INSERT INTO ml.graph_community_stats (tenant_id, project_id, community_id, size, fraud_count, "
                "fraud_rate) VALUES (:t, :p, :community_id, :size, :fraud_count, :fraud_rate)"
            ),
            [{**s, "t": t, "p": p} for s in stats],
        )


def list_communities(conn: Connection, project_id: UUID, min_size: int, limit: int) -> list[dict[str, Any]]:
    rows = (
        conn.execute(
            text(
                """
            SELECT s.community_id, s.size, s.fraud_count, s.fraud_rate, s.computed_at,
                   (SELECT array_agg(c.customer_id::text) FROM (
                        SELECT customer_id FROM ml.graph_communities g
                        WHERE g.project_id = s.project_id AND g.community_id = s.community_id
                        ORDER BY customer_id LIMIT 10) c) AS customer_ids
            FROM ml.graph_community_stats s
            WHERE s.project_id = :p AND s.size >= :min
            ORDER BY s.fraud_count DESC, s.size DESC
            LIMIT :lim
            """
            ),
            {"p": str(project_id), "min": min_size, "lim": limit},
        )
        .mappings()
        .all()
    )
    return [dict(r) | {"customer_ids": r["customer_ids"] or []} for r in rows]


# ------------------------------------------------------------------------------------------ governance
def insert_approval_request(
    conn: Connection,
    tenant_id: UUID,
    project_id: UUID,
    model_id: UUID,
    version: int,
    requested_by: UUID | None,
) -> None:
    conn.execute(
        text(
            "INSERT INTO core.approvals (tenant_id, project_id, subject_type, subject_id, subject_version, "
            "requested_by, target_status) VALUES (:t, :p, 'model', :s, :v, :rb, 'active')"
        ),
        {
            "t": str(tenant_id),
            "p": str(project_id),
            "s": str(model_id),
            "v": version,
            "rb": str(requested_by) if requested_by else None,
        },
    )


def decide_approval(
    conn: Connection, model_id: UUID, decided_by: UUID | None, decision: str, comment: str | None
) -> None:
    conn.execute(
        text(
            """
            UPDATE core.approvals SET decided_by = :db, decided_at = now(), decision = :d, comment = :c
            WHERE id = (SELECT id FROM core.approvals WHERE subject_type = 'model' AND subject_id = :s
                        AND decision IS NULL ORDER BY requested_at DESC LIMIT 1)
            """
        ),
        {"db": str(decided_by) if decided_by else None, "d": decision, "c": comment, "s": str(model_id)},
    )


def audit(
    conn: Connection,
    *,
    tenant_id: UUID | None,
    project_id: UUID | None,
    actor_id: UUID | None,
    actor_type: str,
    action: str,
    subject_type: str,
    subject_id: str,
    before: Any = None,
    after: Any = None,
    metadata: dict[str, Any] | None = None,
    request_id: str | None = None,
) -> None:
    conn.execute(
        text(
            """
            INSERT INTO core.audit_log (tenant_id, project_id, actor_type, actor_id, action, subject_type,
                                        subject_id, before, after, metadata, request_id)
            VALUES (:t, :p, :at, :aid, :action, :st, :sid, CAST(:before AS jsonb), CAST(:after AS jsonb),
                    CAST(:meta AS jsonb), :rid)
            """
        ),
        {
            "t": str(tenant_id) if tenant_id else None,
            "p": str(project_id) if project_id else None,
            "at": actor_type,
            "aid": str(actor_id) if actor_id else None,
            "action": action,
            "st": subject_type,
            "sid": subject_id,
            "before": _json(before) if before is not None else None,
            "after": _json(after) if after is not None else None,
            "meta": _json(metadata or {}),
            "rid": request_id,
        },
    )
