//! Compiles a rule-engine [`VelocityQuery`] into parameterised SQL over `core.events`.
//!
//! ## Safety model (no SQL injection by construction)
//!
//! * **Identifiers** (column names) only come from `contracts::catalog::velocity_column`, which returns
//!   `&'static str` compile-time constants. Nothing typed by a user is ever spliced into SQL text.
//! * `source.*` fields become `e.payload #>> $n::text[]`: the JSON path itself is a **bound parameter**, and
//!   it is only accepted when the project catalogue marks that path velocity-enabled.
//! * **Every value** (group-by values, filter constants, anchor, window, percentile) is a bound parameter.
//!
//! The compiler is a pure function (query + catalogue → SQL text + parameter list), so its output is
//! unit-tested here without a database; the Postgres adapter only binds and executes.
//!
//! ## Query shape
//!
//! ```sql
//! WITH hist AS (
//!   SELECT e.occurred_at AS t, <field as text> AS v_txt, <field as float8> AS v_num
//!   FROM core.events e
//!   WHERE e.project_id = $1 AND e.event_type = ANY($2) AND <group-by> AND <window> [AND e.id <> current]
//!         [AND <history filter>]
//!   [ORDER BY e.occurred_at DESC, e.id DESC LIMIT n]            -- last_n windows
//! )
//! SELECT count(*), <aggregate>, [<values series>] FROM hist
//! ```
//! Bucketed series use a second statement over the same CTE with `generate_series` for zero-filled buckets.
//! Every predicate leads with `project_id` so the `(project_id, <field>, occurred_at DESC)` indexes apply.

use chrono::{DateTime, Utc};
use rule_engine::model::{AggFn, Op};
use rule_engine::ports::{
    HistFilter, HistPredicate, ProviderError, QueryWindow, SeriesRequest, VelocityQuery,
};
use serde_json::Value;
use uuid::Uuid;

use super::catalog::ProjectCatalog;

/// Upper bound on the per-event value series returned to the engine.
pub const MAX_SERIES_VALUES: i64 = 10_000;
/// Upper bound on the number of buckets of a bucketed series.
pub const MAX_BUCKETS: i64 = 10_000;

/// A bound parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Uuid(Uuid),
    Text(String),
    TextArray(Vec<String>),
    Timestamp(DateTime<Utc>),
    Int(i64),
    Float(f64),
}

/// SQL text + parameters, in `$n` order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Statement {
    pub sql: String,
    pub params: Vec<Param>,
}

impl Statement {
    fn bind(&mut self, p: Param) -> String {
        self.params.push(p);
        format!("${}", self.params.len())
    }
}

/// Compiled velocity query.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledVelocity {
    /// Returns one row: `samples int8, aggregate float8 [, vals float8[]]`.
    pub main: Statement,
    pub wants_values: bool,
    /// Returns rows `k int8, start timestamptz, v float8`, oldest bucket first (`k` = buckets back from the anchor).
    pub buckets: Option<(Statement, bool)>,
}

fn invalid(msg: impl Into<String>) -> ProviderError {
    ProviderError::InvalidQuery(msg.into())
}

/// SQL type family of a canonical `core.events` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColType {
    Text,
    Numeric,
    Float,
    Inet,
    Bool,
    Uuid,
}

fn col_type(column: &str) -> ColType {
    match column {
        "amount" | "discount_amount" | "cashback_amount" => ColType::Numeric,
        "latitude" | "longitude" => ColType::Float,
        "ip_address" => ColType::Inet,
        "login_success" => ColType::Bool,
        "customer_id" => ColType::Uuid,
        _ => ColType::Text,
    }
}

fn safe_float(text_expr: &str) -> String {
    format!(
        "(CASE WHEN {text_expr} ~ '^\\s*[-+]?[0-9]+(\\.[0-9]+)?([eE][-+]?[0-9]+)?\\s*$' THEN ({text_expr})::float8 END)"
    )
}

/// A history field resolved to SQL expressions.
#[derive(Debug, Clone)]
struct FieldExpr {
    /// Native column expression (equality/index use) — `None` for `source.*`.
    native: Option<(String, ColType)>,
    /// Value as text.
    text: String,
    /// Value as float8, when numeric-convertible.
    num: Option<String>,
}

/// `items[0].sku` → `["items","0","sku"]`.
fn json_path(path: &str) -> Result<Vec<String>, ProviderError> {
    let mut parts = Vec::new();
    for seg in path.split('.') {
        let mut rest = seg;
        let name_end = rest.find('[').unwrap_or(rest.len());
        let name = &rest[..name_end];
        if name.is_empty() {
            return Err(invalid(format!("invalid source path segment '{seg}'")));
        }
        parts.push(name.to_string());
        rest = &rest[name_end..];
        while let Some(stripped) = rest.strip_prefix('[') {
            let close = stripped
                .find(']')
                .ok_or_else(|| invalid(format!("unclosed index in '{seg}'")))?;
            let idx = &stripped[..close];
            if idx.is_empty() || !idx.chars().all(|c| c.is_ascii_digit()) {
                return Err(invalid(format!("invalid index in '{seg}'")));
            }
            parts.push(idx.to_string());
            rest = &stripped[close + 1..];
        }
        if !rest.is_empty() {
            return Err(invalid(format!("invalid source path segment '{seg}'")));
        }
    }
    Ok(parts)
}

fn resolve_field(
    field: &str,
    catalog: &ProjectCatalog,
    st: &mut Statement,
) -> Result<FieldExpr, ProviderError> {
    if let Some(source_path) = field.strip_prefix("source.") {
        if !catalog.source_velocity_enabled(field) {
            return Err(invalid(format!(
                "field '{field}' is not velocity-enabled for this project"
            )));
        }
        let p = st.bind(Param::TextArray(json_path(source_path)?));
        let text = format!("(e.payload #>> {p}::text[])");
        let num = safe_float(&text);
        return Ok(FieldExpr {
            native: None,
            text,
            num: Some(num),
        });
    }
    let column = contracts::catalog::velocity_column(field)
        .ok_or_else(|| invalid(format!("field '{field}' is not a velocity-enabled column")))?;
    let ty = col_type(column);
    let native = format!("e.{column}");
    let (text, num) = match ty {
        ColType::Text => (native.clone(), Some(safe_float(&native))),
        ColType::Numeric => (format!("{native}::text"), Some(format!("{native}::float8"))),
        ColType::Float => (format!("{native}::text"), Some(native.clone())),
        ColType::Inet => (format!("host({native})"), None),
        ColType::Bool => (
            format!("{native}::text"),
            Some(format!(
                "(CASE WHEN {native} THEN 1.0 WHEN NOT {native} THEN 0.0 END)"
            )),
        ),
        ColType::Uuid => (format!("{native}::text"), None),
    };
    Ok(FieldExpr {
        native: Some((native, ty)),
        text,
        num,
    })
}

fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn check_typed(ty: ColType, raw: &str) -> Result<(), ProviderError> {
    let ok = match ty {
        ColType::Uuid => Uuid::parse_str(raw).is_ok(),
        ColType::Inet => raw
            .split('/')
            .next()
            .is_some_and(|host| host.parse::<std::net::IpAddr>().is_ok()),
        ColType::Numeric | ColType::Float => raw.trim().parse::<f64>().is_ok(),
        ColType::Bool => matches!(raw, "true" | "false"),
        ColType::Text => true,
    };
    if ok {
        Ok(())
    } else {
        Err(invalid(format!("value '{raw}' does not match the column type")))
    }
}

fn cast_suffix(ty: ColType) -> &'static str {
    match ty {
        ColType::Text => "::text",
        ColType::Numeric => "::numeric",
        ColType::Float => "::float8",
        ColType::Inet => "::inet",
        ColType::Bool => "::boolean",
        ColType::Uuid => "::uuid",
    }
}

/// `field = value` using the native column (index friendly) or the source text expression.
fn equality(f: &FieldExpr, value: &Value, negate: bool, st: &mut Statement) -> Result<String, ProviderError> {
    let raw = scalar_text(value).ok_or_else(|| invalid("equality needs a scalar, non-null value"))?;
    let op = if negate { "<>" } else { "=" };
    match &f.native {
        Some((native, ty)) => {
            check_typed(*ty, &raw)?;
            let p = st.bind(Param::Text(raw));
            Ok(format!("{native} {op} {p}{}", cast_suffix(*ty)))
        }
        None => {
            if value.is_number() {
                let num = f.num.as_ref().ok_or_else(|| invalid("field is not numeric"))?;
                let p = st.bind(Param::Float(value.as_f64().unwrap_or(f64::NAN)));
                Ok(format!("{num} {op} {p}::float8"))
            } else {
                let p = st.bind(Param::Text(raw));
                Ok(format!("{} {op} {p}", f.text))
            }
        }
    }
}

fn ordering(f: &FieldExpr, sql_op: &str, value: &Value, st: &mut Statement) -> Result<String, ProviderError> {
    let numeric_col = matches!(
        f.native.as_ref().map(|(_, t)| *t),
        Some(ColType::Numeric | ColType::Float)
    );
    if value.is_number() || numeric_col {
        let num = f
            .num
            .as_ref()
            .ok_or_else(|| invalid("ordering operator on a non-numeric field"))?;
        let x = match value {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| invalid("expected a number"))?,
            _ => return Err(invalid("expected a number")),
        };
        let p = st.bind(Param::Float(x));
        Ok(format!("{num} {sql_op} {p}::float8"))
    } else if let Value::String(s) = value {
        if matches!(
            f.native.as_ref().map(|(_, t)| *t),
            Some(ColType::Uuid | ColType::Inet | ColType::Bool)
        ) {
            return Err(invalid("ordering operator on a non-orderable field"));
        }
        let p = st.bind(Param::Text(s.clone()));
        Ok(format!("{} {sql_op} {p}", f.text))
    } else {
        Err(invalid("ordering operator needs a number or string"))
    }
}

fn membership(
    f: &FieldExpr,
    value: &Value,
    negate: bool,
    st: &mut Statement,
) -> Result<String, ProviderError> {
    let items = value
        .as_array()
        .ok_or_else(|| invalid("in/not_in needs an array"))?;
    let raw: Vec<String> = items
        .iter()
        .map(|v| scalar_text(v).ok_or_else(|| invalid("in/not_in array must contain scalars")))
        .collect::<Result<_, _>>()?;
    let expr = match &f.native {
        Some((native, ty)) => {
            for r in &raw {
                check_typed(*ty, r)?;
            }
            let p = st.bind(Param::TextArray(raw));
            format!("{native} = ANY({p}::text[]{}[])", cast_suffix(*ty))
        }
        None if !items.is_empty() && items.iter().all(Value::is_number) => {
            let num = f.num.as_ref().ok_or_else(|| invalid("field is not numeric"))?;
            let p = st.bind(Param::TextArray(raw));
            format!("{num} = ANY({p}::text[]::float8[])")
        }
        None => {
            let p = st.bind(Param::TextArray(raw));
            format!("{} = ANY({p}::text[])", f.text)
        }
    };
    Ok(if negate {
        format!("({} IS NOT NULL AND NOT ({expr}))", f.text)
    } else {
        expr
    })
}

fn predicate(
    p: &HistPredicate,
    catalog: &ProjectCatalog,
    st: &mut Statement,
) -> Result<String, ProviderError> {
    if !p.op.allowed_in_history_filter() {
        return Err(invalid(format!(
            "operator '{}' is not allowed in history filters",
            p.op.as_str()
        )));
    }
    let f = resolve_field(&p.field, catalog, st)?;
    match p.op {
        Op::Eq => equality(&f, &p.value, false, st),
        Op::Ne => equality(&f, &p.value, true, st),
        Op::Gt => ordering(&f, ">", &p.value, st),
        Op::Gte => ordering(&f, ">=", &p.value, st),
        Op::Lt => ordering(&f, "<", &p.value, st),
        Op::Lte => ordering(&f, "<=", &p.value, st),
        Op::Between => {
            let arr = p
                .value
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| invalid("between needs [lo, hi]"))?;
            let lo = ordering(&f, ">=", &arr[0], st)?;
            let hi = ordering(&f, "<=", &arr[1], st)?;
            Ok(format!("({lo} AND {hi})"))
        }
        Op::In => membership(&f, &p.value, false, st),
        Op::NotIn => membership(&f, &p.value, true, st),
        Op::IsNull => Ok(format!("{} IS NULL", f.text)),
        Op::IsNotNull => Ok(format!("{} IS NOT NULL", f.text)),
        Op::StartsWith | Op::Contains => {
            let s = p
                .value
                .as_str()
                .ok_or_else(|| invalid("starts_with/contains needs a string"))?;
            let param = st.bind(Param::Text(s.to_string()));
            Ok(if p.op == Op::StartsWith {
                format!("starts_with({}, {param})", f.text)
            } else {
                format!("strpos({}, {param}) > 0", f.text)
            })
        }
        other => Err(invalid(format!(
            "operator '{}' is not supported in SQL",
            other.as_str()
        ))),
    }
}

fn filter_sql(
    filter: &HistFilter,
    catalog: &ProjectCatalog,
    st: &mut Statement,
) -> Result<String, ProviderError> {
    Ok(match filter {
        HistFilter::And(items) if items.is_empty() => "TRUE".to_string(),
        HistFilter::Or(items) if items.is_empty() => "FALSE".to_string(),
        HistFilter::And(items) => {
            let parts = items
                .iter()
                .map(|f| filter_sql(f, catalog, st))
                .collect::<Result<Vec<_>, _>>()?;
            format!("({})", parts.join(" AND "))
        }
        HistFilter::Or(items) => {
            let parts = items
                .iter()
                .map(|f| filter_sql(f, catalog, st))
                .collect::<Result<Vec<_>, _>>()?;
            format!("({})", parts.join(" OR "))
        }
        HistFilter::Not(inner) => format!("(NOT {})", filter_sql(inner, catalog, st)?),
        HistFilter::Pred(p) => predicate(p, catalog, st)?,
    })
}

/// Aggregate expression over the `hist` CTE columns `v_txt` / `v_num`.
fn aggregate_expr(
    func: AggFn,
    has_field: bool,
    has_num: bool,
    percentile: Option<f64>,
    st: &mut Statement,
) -> Result<String, ProviderError> {
    let need_num = |e: &str| -> Result<String, ProviderError> {
        if has_num {
            Ok(e.to_string())
        } else {
            Err(invalid(format!(
                "aggregate '{}' needs a numeric field",
                func.as_str()
            )))
        }
    };
    match func {
        AggFn::Count => Ok(if has_field {
            "count(v_txt)".into()
        } else {
            "count(*)".into()
        }),
        AggFn::DistinctCount => {
            if !has_field {
                return Err(invalid("distinct_count needs a field"));
            }
            Ok("count(DISTINCT v_txt)".into())
        }
        AggFn::Sum => need_num("COALESCE(sum(v_num), 0)"),
        AggFn::Avg => need_num("avg(v_num)"),
        AggFn::Min => need_num("min(v_num)"),
        AggFn::Max => need_num("max(v_num)"),
        AggFn::Stddev => need_num("stddev_samp(v_num)"),
        AggFn::Median => need_num("percentile_cont(0.5) WITHIN GROUP (ORDER BY v_num)"),
        AggFn::Percentile => {
            let p = percentile
                .filter(|p| *p > 0.0 && *p < 1.0)
                .ok_or_else(|| invalid("percentile needs p in (0,1)"))?;
            let param = st.bind(Param::Float(p));
            need_num(&format!(
                "percentile_cont({param}::float8) WITHIN GROUP (ORDER BY v_num)"
            ))
        }
    }
}

fn zero_filled(func: AggFn) -> bool {
    matches!(func, AggFn::Count | AggFn::Sum | AggFn::DistinctCount)
}

/// Builds the `hist` CTE into `st` and returns `(cte_sql, has_field, has_num)`.
fn hist_cte(
    q: &VelocityQuery,
    project: Uuid,
    catalog: &ProjectCatalog,
    st: &mut Statement,
) -> Result<(String, bool, bool, Option<String>), ProviderError> {
    if q.group_by.is_empty() {
        return Err(invalid("group_by must not be empty"));
    }
    if q.history_event_types.is_empty() {
        return Err(invalid("history_event_types must not be empty"));
    }
    let p_project = st.bind(Param::Uuid(project));
    let p_types = st.bind(Param::TextArray(q.history_event_types.clone()));

    let (v_txt, v_num, has_field, has_num) = match &q.aggregate_field {
        Some(field) => {
            let f = resolve_field(field, catalog, st)?;
            let has_num = f.num.is_some();
            (
                f.text.clone(),
                f.num.unwrap_or_else(|| "NULL::float8".into()),
                true,
                has_num,
            )
        }
        None => ("NULL::text".into(), "NULL::float8".into(), false, false),
    };

    let mut conds = vec![
        format!("e.project_id = {p_project}"),
        format!("e.event_type = ANY({p_types}::text[])"),
    ];
    for key in &q.group_by {
        if key.value.is_null() {
            return Err(invalid(format!("group_by value of '{}' is null", key.field)));
        }
        let f = resolve_field(&key.field, catalog, st)?;
        conds.push(equality(&f, &key.value, false, st)?);
    }
    let p_anchor = st.bind(Param::Timestamp(q.anchor));
    let mut tail = String::new();
    match &q.window {
        QueryWindow::Duration { seconds } => {
            if *seconds <= 0 {
                return Err(invalid("window must be positive"));
            }
            let p_secs = st.bind(Param::Int(*seconds));
            conds.push(format!(
                "e.occurred_at > {p_anchor} - ({p_secs}::float8 * interval '1 second') AND e.occurred_at <= {p_anchor}"
            ));
        }
        QueryWindow::LastN { n } => {
            if *n == 0 {
                return Err(invalid("last_n must be positive"));
            }
            conds.push(format!("e.occurred_at <= {p_anchor}"));
            let p_n = st.bind(Param::Int(i64::from(*n)));
            tail = format!(" ORDER BY e.occurred_at DESC, e.id DESC LIMIT {p_n}");
        }
    }
    if !q.include_current {
        if let Some(id) = q.event_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
            let p = st.bind(Param::Uuid(id));
            conds.push(format!("e.id <> {p}"));
        }
    }
    if let Some(filter) = &q.filter {
        conds.push(filter_sql(filter, catalog, st)?);
    }
    let cte = format!(
        "WITH hist AS (SELECT e.occurred_at AS t, {v_txt} AS v_txt, {v_num} AS v_num FROM core.events e WHERE {}{tail})",
        conds.join(" AND ")
    );
    Ok((cte, has_field, has_num, Some(p_anchor)))
}

/// Compiles a velocity query for `project`.
pub fn compile(
    q: &VelocityQuery,
    project: Uuid,
    catalog: &ProjectCatalog,
) -> Result<CompiledVelocity, ProviderError> {
    let mut main = Statement::default();
    let (cte, has_field, has_num, _) = hist_cte(q, project, catalog, &mut main)?;
    let agg = aggregate_expr(q.aggregate_fn, has_field, has_num, q.percentile, &mut main)?;
    let wants_values = matches!(q.series, SeriesRequest::Values);
    let values = if wants_values {
        if !has_num {
            return Err(invalid("a value series needs a numeric aggregate field"));
        }
        format!(
            ", COALESCE((SELECT array_agg(v_num ORDER BY t) FROM (SELECT t, v_num FROM hist WHERE v_num IS NOT NULL \
             ORDER BY t DESC LIMIT {MAX_SERIES_VALUES}) s), '{{}}')::float8[] AS vals"
        )
    } else {
        String::new()
    };
    main.sql =
        format!("{cte} SELECT count(*)::int8 AS samples, ({agg})::float8 AS aggregate{values} FROM hist");

    let buckets = match &q.series {
        SeriesRequest::Buckets { bucket_seconds, func } => {
            let &QueryWindow::Duration { seconds } = &q.window else {
                return Err(invalid("bucketed series need a duration window"));
            };
            if *bucket_seconds <= 0 || *bucket_seconds > seconds {
                return Err(invalid("bucket must be positive and not longer than the window"));
            }
            let count = (seconds + bucket_seconds - 1) / bucket_seconds;
            if count > MAX_BUCKETS {
                return Err(invalid("too many buckets"));
            }
            let mut st = Statement::default();
            let (cte, has_field, has_num, anchor) = hist_cte(q, project, catalog, &mut st)?;
            let anchor = anchor.unwrap_or_default();
            let agg = aggregate_expr(*func, has_field, has_num, q.percentile, &mut st)?;
            let p_b = st.bind(Param::Int(*bucket_seconds));
            let p_nb = st.bind(Param::Int(count));
            st.sql = format!(
                "{cte}, b AS (SELECT floor(extract(epoch FROM ({anchor} - t)) / {p_b}::float8)::int8 AS k, v_txt, v_num FROM hist), \
                 agg AS (SELECT k, ({agg})::float8 AS v FROM b GROUP BY k) \
                 SELECT g.k::int8 AS k, ({anchor} - ((g.k + 1) * {p_b}::float8) * interval '1 second') AS start, agg.v AS v \
                 FROM generate_series(0::int8, {p_nb}::int8 - 1) AS g(k) LEFT JOIN agg ON agg.k = g.k ORDER BY g.k DESC"
            );
            Some((st, zero_filled(*func)))
        }
        _ => None,
    };
    Ok(CompiledVelocity {
        main,
        wants_values,
        buckets,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rule_engine::ports::GroupKey;
    use serde_json::json;

    fn base() -> VelocityQuery {
        VelocityQuery {
            event_id: Some(Uuid::nil().to_string()),
            anchor: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
            history_event_types: vec!["transaction".into()],
            group_by: vec![GroupKey {
                field: "instrument_fingerprint".into(),
                value: json!("abc"),
            }],
            window: QueryWindow::Duration { seconds: 86_400 },
            aggregate_fn: AggFn::DistinctCount,
            aggregate_field: Some("customer_id".into()),
            percentile: None,
            include_current: true,
            filter: None,
            series: SeriesRequest::None,
        }
    }

    fn catalog() -> ProjectCatalog {
        ProjectCatalog::with_source_fields([
            ("source.order.total".into(), true),
            ("source.secret".into(), false),
        ])
    }

    #[test]
    fn plain_query_binds_every_value() {
        let c = compile(&base(), Uuid::nil(), &catalog()).unwrap();
        let sql = &c.main.sql;
        assert!(sql.contains("e.project_id = $1"), "{sql}");
        assert!(sql.contains("e.event_type = ANY($2::text[])"));
        assert!(sql.contains("e.instrument_fingerprint = $3::text"));
        assert!(sql.contains("count(DISTINCT v_txt)"));
        assert!(!sql.contains("abc"), "values must never be inlined: {sql}");
        assert!(
            !sql.contains("e.id <>"),
            "include_current keeps the current event"
        );
        assert_eq!(c.main.params[2], Param::Text("abc".into()));
    }

    #[test]
    fn exclude_current_and_last_n() {
        let mut q = base();
        q.include_current = false;
        q.window = QueryWindow::LastN { n: 20 };
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        assert!(c.main.sql.contains("e.id <> $"));
        assert!(c
            .main
            .sql
            .contains("ORDER BY e.occurred_at DESC, e.id DESC LIMIT $"));
    }

    #[test]
    fn source_field_path_is_a_parameter() {
        let mut q = base();
        q.aggregate_fn = AggFn::Sum;
        q.aggregate_field = Some("source.order.total".into());
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        assert!(c.main.sql.contains("e.payload #>> $"), "{}", c.main.sql);
        assert!(c
            .main
            .params
            .contains(&Param::TextArray(vec!["order".into(), "total".into()])));
    }

    #[test]
    fn injection_attempts_and_disallowed_fields_are_rejected() {
        for field in [
            "amount; DROP TABLE core.events",
            "card_last4",
            "source.secret",
            "source.unknown",
            "features.cust_cnt_1h",
            "e.amount",
        ] {
            let mut q = base();
            q.aggregate_field = Some(field.into());
            assert!(
                matches!(
                    compile(&q, Uuid::nil(), &catalog()),
                    Err(ProviderError::InvalidQuery(_))
                ),
                "{field} must be rejected"
            );
        }
        let mut q = base();
        q.group_by = vec![GroupKey {
            field: "device_id') OR 1=1 --".into(),
            value: json!("x"),
        }];
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
    }

    #[test]
    fn history_filter_compiles_all_supported_ops() {
        let pred = |field: &str, op: Op, value: Value| {
            HistFilter::Pred(HistPredicate {
                field: field.into(),
                op,
                value,
            })
        };
        let mut q = base();
        q.filter = Some(HistFilter::And(vec![
            pred("promo_code", Op::Eq, json!("NEWUSER")),
            pred("discount_amount", Op::Gt, json!(0)),
            HistFilter::Or(vec![
                pred("channel", Op::In, json!(["web", "api"])),
                HistFilter::Not(Box::new(pred("status", Op::StartsWith, json!("fail")))),
            ]),
            pred("amount", Op::Between, json!([10, 100])),
            pred("login_success", Op::Eq, json!(false)),
            pred("geo_country", Op::NotIn, json!(["ID"])),
            pred("source.order.total", Op::Gte, json!(5)),
            pred("merchant_id", Op::IsNotNull, Value::Null),
            pred("geo_city", Op::Contains, json!("Jakarta")),
        ]));
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        let sql = &c.main.sql;
        assert!(sql.contains("e.promo_code = $"), "{sql}");
        assert!(sql.contains("e.discount_amount::float8 > $"));
        assert!(sql.contains("e.channel = ANY($"));
        assert!(sql.contains("NOT starts_with(e.status, $"));
        assert!(sql.contains("e.login_success = $") && sql.contains("::boolean"));
        assert!(sql.contains("strpos(e.geo_city, $"));
        assert!(!sql.contains("NEWUSER") && !sql.contains("Jakarta"));
    }

    #[test]
    fn regex_or_similar_in_history_filter_is_rejected() {
        let mut q = base();
        q.filter = Some(HistFilter::Pred(HistPredicate {
            field: "status".into(),
            op: Op::Regex,
            value: json!(".*"),
        }));
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
    }

    #[test]
    fn typed_values_are_checked() {
        let mut q = base();
        q.group_by = vec![GroupKey {
            field: "customer_id".into(),
            value: json!("not-a-uuid"),
        }];
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
        q.group_by = vec![GroupKey {
            field: "customer_id".into(),
            value: json!(Uuid::nil().to_string()),
        }];
        assert!(compile(&q, Uuid::nil(), &catalog()).is_ok());
        q.group_by = vec![GroupKey {
            field: "device_id".into(),
            value: Value::Null,
        }];
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
    }

    #[test]
    fn numeric_aggregates_need_numeric_fields() {
        let mut q = base();
        q.aggregate_fn = AggFn::Avg;
        q.aggregate_field = Some("customer_id".into());
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
        q.aggregate_field = Some("amount".into());
        q.aggregate_fn = AggFn::Percentile;
        q.percentile = Some(0.9);
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        assert!(c.main.sql.contains("percentile_cont($"));
    }

    #[test]
    fn values_and_buckets() {
        let mut q = base();
        q.aggregate_fn = AggFn::Avg;
        q.aggregate_field = Some("amount".into());
        q.series = SeriesRequest::Values;
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        assert!(c.wants_values && c.main.sql.contains("AS vals"));

        q.series = SeriesRequest::Buckets {
            bucket_seconds: 3_600,
            func: AggFn::Count,
        };
        let c = compile(&q, Uuid::nil(), &catalog()).unwrap();
        let (st, zero) = c.buckets.unwrap();
        assert!(zero);
        assert!(st.sql.contains("generate_series"));
        assert!(st.params.contains(&Param::Int(24)));

        q.window = QueryWindow::LastN { n: 5 };
        assert!(compile(&q, Uuid::nil(), &catalog()).is_err());
    }

    #[test]
    fn json_paths() {
        assert_eq!(json_path("items[0].sku").unwrap(), vec!["items", "0", "sku"]);
        assert_eq!(json_path("a.b").unwrap(), vec!["a", "b"]);
        assert!(json_path("a..b").is_err());
        assert!(json_path("a[x]").is_err());
    }
}
