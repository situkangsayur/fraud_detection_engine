//! Reference lists: project-scoped (`/api/v1/projects/{pid}/reference-lists`) and tenant-wide
//! (`/api/v1/tenants/{tid}/reference-lists`). One set of handler bodies serves both scopes; the scope is
//! resolved (and authorised) first, then everything else is identical.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::error::FieldError;
use platform::pagination::{Page, PageParams};
use platform::{AppError, AppResult, ProjectId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::adapters::repo::{self, EntryRowDb, ListRow, ListScope};
use crate::domain::csv_import::{self, EntryRow, RowError, MAX_KEY_LEN};
use crate::state::AppState;

const LIST_TYPES: &[&str] = &["blacklist", "whitelist", "watchlist", "lookup"];
const COLUMN_TYPES: &[&str] = &["string", "number", "integer", "bool", "boolean", "datetime"];
const MAX_ENTRIES_PER_REQUEST: usize = 10_000;
const IMPORT_BATCH: usize = 1_000;

#[derive(Debug, Clone, Copy)]
enum Access {
    Read,
    Write,
}

async fn project_list_scope(caller: &Caller, pid: Uuid, access: Access) -> AppResult<ListScope> {
    let role = match access {
        Access::Read => ProjectRole::Viewer,
        Access::Write => ProjectRole::Analyst,
    };
    let tenant = caller.require_project_role(ProjectId(pid), role).await?;
    Ok(ListScope::Project(tenant, ProjectId(pid)))
}

fn tenant_list_scope(caller: &Caller, tid: Uuid, access: Access) -> AppResult<ListScope> {
    let tenant = TenantId(tid);
    match access {
        Access::Read => caller.require_tenant_member(tenant)?,
        Access::Write => caller.require_tenant_admin(tenant)?,
    }
    Ok(ListScope::Tenant(tenant))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListOut {
    #[serde(flatten)]
    pub list: ListRow,
    /// `project` or `tenant` (tenant-wide lists are visible, read-only, in every project).
    pub scope: &'static str,
}

impl From<ListRow> for ListOut {
    fn from(list: ListRow) -> Self {
        let scope = list.scope();
        Self { list, scope }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateListBody {
    pub name: String,
    pub description: Option<String>,
    pub list_type: String,
    #[serde(default)]
    pub key_kind: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub columns: Option<Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchListBody {
    pub description: Option<String>,
    pub list_type: Option<String>,
    pub key_kind: Option<String>,
    #[schema(value_type = Object)]
    pub columns: Option<Value>,
}

fn check_list_type(t: &str, errors: &mut Vec<FieldError>) {
    if !LIST_TYPES.contains(&t) {
        errors.push(FieldError::new(
            "list_type",
            format!("must be one of {}", LIST_TYPES.join(", ")),
        ));
    }
}

fn check_columns(columns: &Value, errors: &mut Vec<FieldError>) {
    let Some(arr) = columns.as_array() else {
        errors.push(FieldError::new("columns", "must be an array of {name, type}"));
        return;
    };
    for (i, c) in arr.iter().enumerate() {
        let name_ok = c
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| !n.is_empty() && n.len() <= 64);
        let type_ok = c
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| COLUMN_TYPES.contains(&t));
        if !name_ok || !type_ok {
            errors.push(FieldError::new(
                format!("columns[{i}]"),
                format!("needs a name and a type in {}", COLUMN_TYPES.join(", ")),
            ));
        }
    }
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase() | bytes[0].is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

// ---- shared bodies -------------------------------------------------------------------------------------------

async fn do_list(state: &AppState, scope: ListScope) -> AppResult<Vec<ListOut>> {
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    let rows = repo::list_lists(&mut tx, scope).await?;
    tx.commit().await?;
    Ok(rows.into_iter().map(ListOut::from).collect())
}

async fn do_create(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    body: CreateListBody,
) -> AppResult<ListOut> {
    let mut errors = Vec::new();
    if !valid_name(&body.name) {
        errors.push(FieldError::new(
            "name",
            "lower-case letters, digits and '_' (2-63 chars)",
        ));
    }
    check_list_type(&body.list_type, &mut errors);
    let columns = body.columns.unwrap_or_else(|| serde_json::json!([]));
    check_columns(&columns, &mut errors);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    if repo::find_list_by_name(&mut tx, scope, &body.name)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(format!(
            "a list named '{}' already exists",
            body.name
        )));
    }
    let id = repo::insert_list(
        &mut tx,
        scope,
        &body.name,
        body.description.as_deref(),
        &body.list_type,
        body.key_kind.as_deref().unwrap_or("generic"),
        &columns,
        caller.actor_user_id().map(|u| u.0),
    )
    .await?;
    let row = repo::get_list(&mut tx, scope, id, false).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "reference_list.create")
            .scope(scope.tenant(), scope.project())
            .subject("reference_list", id)
            .after(&row),
    )
    .await?;
    tx.commit().await?;
    Ok(row.into())
}

async fn do_patch(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    id: Uuid,
    body: PatchListBody,
) -> AppResult<ListOut> {
    let mut errors = Vec::new();
    if let Some(t) = &body.list_type {
        check_list_type(t, &mut errors);
    }
    if let Some(c) = &body.columns {
        check_columns(c, &mut errors);
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    let before = repo::get_list(&mut tx, scope, id, false).await?;
    repo::update_list(
        &mut tx,
        id,
        body.description.as_deref(),
        body.list_type.as_deref(),
        body.key_kind.as_deref(),
        body.columns.as_ref(),
    )
    .await?;
    let after = repo::get_list(&mut tx, scope, id, false).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "reference_list.update")
            .scope(scope.tenant(), scope.project())
            .subject("reference_list", id)
            .before(&before)
            .after(&after),
    )
    .await?;
    tx.commit().await?;
    state.ref_cache.invalidate_all();
    Ok(after.into())
}

async fn do_delete(state: &AppState, caller: &Caller, scope: ListScope, id: Uuid) -> AppResult<StatusCode> {
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    let list = repo::get_list(&mut tx, scope, id, false).await?;
    let users = repo::rules_referencing_list(&mut tx, scope, &list.name).await?;
    if !users.is_empty() {
        return Err(AppError::Conflict(format!(
            "list is referenced by rules {}; retire or change them first",
            users.join(", ")
        )));
    }
    repo::delete_list(&mut tx, id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "reference_list.delete")
            .scope(scope.tenant(), scope.project())
            .subject("reference_list", id)
            .before(&list),
    )
    .await?;
    tx.commit().await?;
    state.ref_cache.invalidate_all();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EntriesQuery {
    pub q: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

async fn do_entries(
    state: &AppState,
    scope: ListScope,
    id: Uuid,
    q: EntriesQuery,
) -> AppResult<Page<EntryRowDb>> {
    let page = PageParams {
        page: q.page,
        page_size: q.page_size,
    };
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    repo::get_list(&mut tx, scope, id, true).await?;
    let (rows, total) = repo::list_entries(&mut tx, id, q.q.as_deref(), page.limit(), page.offset()).await?;
    tx.commit().await?;
    Ok(Page::new(rows, total, &page))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryIn {
    pub key: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: Option<Value>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntriesBody {
    pub entries: Vec<EntryIn>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpsertOut {
    pub upserted: u64,
    pub errors: Vec<RowError>,
}

async fn upsert_rows(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    id: Uuid,
    rows: Vec<EntryRow>,
    action: &str,
) -> AppResult<u64> {
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    repo::get_list(&mut tx, scope, id, false).await?;
    let mut total = 0;
    for chunk in rows.chunks(IMPORT_BATCH) {
        total += repo::upsert_entries(
            &mut tx,
            scope.tenant(),
            id,
            chunk,
            caller.actor_user_id().map(|u| u.0),
        )
        .await?;
    }
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, action)
            .scope(scope.tenant(), scope.project())
            .subject("reference_list", id)
            .metadata(serde_json::json!({ "upserted": total })),
    )
    .await?;
    tx.commit().await?;
    state.ref_cache.invalidate_all();
    Ok(total)
}

async fn do_upsert(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    id: Uuid,
    body: EntriesBody,
) -> AppResult<UpsertOut> {
    if body.entries.len() > MAX_ENTRIES_PER_REQUEST {
        return Err(AppError::field(
            "entries",
            format!("at most {MAX_ENTRIES_PER_REQUEST} entries per request"),
        ));
    }
    let mut errors = Vec::new();
    let mut rows = Vec::with_capacity(body.entries.len());
    for (i, e) in body.entries.into_iter().enumerate() {
        let key = e.key.trim().to_string();
        let attributes = e.attributes.unwrap_or_else(|| serde_json::json!({}));
        let problem = if key.is_empty() {
            Some("empty key")
        } else if key.len() > MAX_KEY_LEN {
            Some("key too long")
        } else if !attributes.is_object() {
            Some("attributes must be an object")
        } else if matches!((e.valid_from, e.valid_until), (Some(f), Some(u)) if u <= f) {
            Some("valid_until must be after valid_from")
        } else {
            None
        };
        match problem {
            Some(message) => errors.push(FieldError::new(format!("entries[{i}]"), message)),
            None => rows.push(EntryRow {
                key,
                attributes,
                valid_from: e.valid_from,
                valid_until: e.valid_until,
                reason: e.reason,
            }),
        }
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let upserted = upsert_rows(state, caller, scope, id, rows, "reference_list.entries.upsert").await?;
    Ok(UpsertOut {
        upserted,
        errors: vec![],
    })
}

async fn do_import(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    id: Uuid,
    mut mp: Multipart,
) -> AppResult<UpsertOut> {
    let mut data = Vec::new();
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        if field.name() == Some("file") {
            data = field
                .bytes()
                .await
                .map_err(|e| AppError::BadRequest(format!("cannot read upload: {e}")))?
                .to_vec();
        }
    }
    if data.is_empty() {
        return Err(AppError::field("file", "a non-empty CSV file is required"));
    }
    let columns = {
        let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
        let list = repo::get_list(&mut tx, scope, id, false).await?;
        tx.commit().await?;
        list.columns
    };
    let (rows, errors) = csv_import::parse(&data, &columns).map_err(|m| AppError::field("file", m))?;
    let upserted = upsert_rows(state, caller, scope, id, rows, "reference_list.import").await?;
    Ok(UpsertOut { upserted, errors })
}

async fn do_delete_entry(
    state: &AppState,
    caller: &Caller,
    scope: ListScope,
    id: Uuid,
    entry_id: i64,
) -> AppResult<StatusCode> {
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    repo::get_list(&mut tx, scope, id, false).await?;
    repo::delete_entry(&mut tx, id, entry_id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "reference_list.entry.delete")
            .scope(scope.tenant(), scope.project())
            .subject("reference_list", id)
            .metadata(serde_json::json!({ "entry_id": entry_id })),
    )
    .await?;
    tx.commit().await?;
    state.ref_cache.invalidate_all();
    Ok(StatusCode::NO_CONTENT)
}

async fn do_get(state: &AppState, scope: ListScope, id: Uuid) -> AppResult<ListOut> {
    let mut tx = TenantTx::begin(&state.pool, scope.tenant()).await?;
    let row = repo::get_list(&mut tx, scope, id, true).await?;
    tx.commit().await?;
    Ok(row.into())
}

// ---- project-scoped routes -----------------------------------------------------------------------------------

#[utoipa::path(get, path = "/api/v1/projects/{pid}/reference-lists", params(("pid" = Uuid, Path)),
    responses((status = 200, body = Vec<ListOut>)), tag = "reference-lists")]
pub async fn project_list(
    State(s): State<AppState>,
    c: Caller,
    Path(pid): Path<Uuid>,
) -> AppResult<Json<Vec<ListOut>>> {
    let scope = project_list_scope(&c, pid, Access::Read).await?;
    Ok(Json(do_list(&s, scope).await?))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/reference-lists", params(("pid" = Uuid, Path)),
    request_body = CreateListBody, responses((status = 201, body = ListOut)), tag = "reference-lists")]
pub async fn project_create(
    State(s): State<AppState>,
    c: Caller,
    Path(pid): Path<Uuid>,
    Json(b): Json<CreateListBody>,
) -> AppResult<(StatusCode, Json<ListOut>)> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    Ok((StatusCode::CREATED, Json(do_create(&s, &c, scope, b).await?)))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/reference-lists/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = ListOut)), tag = "reference-lists")]
pub async fn project_get(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ListOut>> {
    let scope = project_list_scope(&c, pid, Access::Read).await?;
    Ok(Json(do_get(&s, scope, id).await?))
}

#[utoipa::path(patch, path = "/api/v1/projects/{pid}/reference-lists/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = PatchListBody, responses((status = 200, body = ListOut)), tag = "reference-lists")]
pub async fn project_patch(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Json(b): Json<PatchListBody>,
) -> AppResult<Json<ListOut>> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    Ok(Json(do_patch(&s, &c, scope, id, b).await?))
}

#[utoipa::path(delete, path = "/api/v1/projects/{pid}/reference-lists/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 204), (status = 409, description = "Referenced by rules")), tag = "reference-lists")]
pub async fn project_delete(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    do_delete(&s, &c, scope, id).await
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/reference-lists/{id}/entries",
    params(("pid" = Uuid, Path), ("id" = Uuid, Path), EntriesQuery), responses((status = 200, description = "Entries page")),
    tag = "reference-lists")]
pub async fn project_entries(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<EntriesQuery>,
) -> AppResult<Json<Page<EntryRowDb>>> {
    let scope = project_list_scope(&c, pid, Access::Read).await?;
    Ok(Json(do_entries(&s, scope, id, q).await?))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/reference-lists/{id}/entries",
    params(("pid" = Uuid, Path), ("id" = Uuid, Path)), request_body = EntriesBody, responses((status = 200, body = UpsertOut)),
    tag = "reference-lists")]
pub async fn project_upsert(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Json(b): Json<EntriesBody>,
) -> AppResult<Json<UpsertOut>> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    Ok(Json(do_upsert(&s, &c, scope, id, b).await?))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/reference-lists/{id}/import",
    params(("pid" = Uuid, Path), ("id" = Uuid, Path)), request_body(content_type = "multipart/form-data", content = String),
    responses((status = 200, body = UpsertOut)), tag = "reference-lists")]
pub async fn project_import(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    mp: Multipart,
) -> AppResult<Json<UpsertOut>> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    Ok(Json(do_import(&s, &c, scope, id, mp).await?))
}

#[utoipa::path(delete, path = "/api/v1/projects/{pid}/reference-lists/{id}/entries/{entry_id}",
    params(("pid" = Uuid, Path), ("id" = Uuid, Path), ("entry_id" = i64, Path)), responses((status = 204)),
    tag = "reference-lists")]
pub async fn project_delete_entry(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id, entry_id)): Path<(Uuid, Uuid, i64)>,
) -> AppResult<StatusCode> {
    let scope = project_list_scope(&c, pid, Access::Write).await?;
    do_delete_entry(&s, &c, scope, id, entry_id).await
}

// ---- tenant-wide routes --------------------------------------------------------------------------------------

#[utoipa::path(get, path = "/api/v1/tenants/{tid}/reference-lists", params(("tid" = Uuid, Path)),
    responses((status = 200, body = Vec<ListOut>)), tag = "reference-lists")]
pub async fn tenant_list(
    State(s): State<AppState>,
    c: Caller,
    Path(tid): Path<Uuid>,
) -> AppResult<Json<Vec<ListOut>>> {
    let scope = tenant_list_scope(&c, tid, Access::Read)?;
    Ok(Json(do_list(&s, scope).await?))
}

#[utoipa::path(post, path = "/api/v1/tenants/{tid}/reference-lists", params(("tid" = Uuid, Path)),
    request_body = CreateListBody, responses((status = 201, body = ListOut)), tag = "reference-lists")]
pub async fn tenant_create(
    State(s): State<AppState>,
    c: Caller,
    Path(tid): Path<Uuid>,
    Json(b): Json<CreateListBody>,
) -> AppResult<(StatusCode, Json<ListOut>)> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    Ok((StatusCode::CREATED, Json(do_create(&s, &c, scope, b).await?)))
}

#[utoipa::path(get, path = "/api/v1/tenants/{tid}/reference-lists/{id}", params(("tid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = ListOut)), tag = "reference-lists")]
pub async fn tenant_get(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ListOut>> {
    let scope = tenant_list_scope(&c, tid, Access::Read)?;
    Ok(Json(do_get(&s, scope, id).await?))
}

#[utoipa::path(patch, path = "/api/v1/tenants/{tid}/reference-lists/{id}", params(("tid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = PatchListBody, responses((status = 200, body = ListOut)), tag = "reference-lists")]
pub async fn tenant_patch(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
    Json(b): Json<PatchListBody>,
) -> AppResult<Json<ListOut>> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    Ok(Json(do_patch(&s, &c, scope, id, b).await?))
}

#[utoipa::path(delete, path = "/api/v1/tenants/{tid}/reference-lists/{id}", params(("tid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 204)), tag = "reference-lists")]
pub async fn tenant_delete(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    do_delete(&s, &c, scope, id).await
}

#[utoipa::path(get, path = "/api/v1/tenants/{tid}/reference-lists/{id}/entries",
    params(("tid" = Uuid, Path), ("id" = Uuid, Path), EntriesQuery), responses((status = 200, description = "Entries page")),
    tag = "reference-lists")]
pub async fn tenant_entries(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<EntriesQuery>,
) -> AppResult<Json<Page<EntryRowDb>>> {
    let scope = tenant_list_scope(&c, tid, Access::Read)?;
    Ok(Json(do_entries(&s, scope, id, q).await?))
}

#[utoipa::path(post, path = "/api/v1/tenants/{tid}/reference-lists/{id}/entries",
    params(("tid" = Uuid, Path), ("id" = Uuid, Path)), request_body = EntriesBody, responses((status = 200, body = UpsertOut)),
    tag = "reference-lists")]
pub async fn tenant_upsert(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
    Json(b): Json<EntriesBody>,
) -> AppResult<Json<UpsertOut>> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    Ok(Json(do_upsert(&s, &c, scope, id, b).await?))
}

#[utoipa::path(post, path = "/api/v1/tenants/{tid}/reference-lists/{id}/import",
    params(("tid" = Uuid, Path), ("id" = Uuid, Path)), request_body(content_type = "multipart/form-data", content = String),
    responses((status = 200, body = UpsertOut)), tag = "reference-lists")]
pub async fn tenant_import(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id)): Path<(Uuid, Uuid)>,
    mp: Multipart,
) -> AppResult<Json<UpsertOut>> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    Ok(Json(do_import(&s, &c, scope, id, mp).await?))
}

#[utoipa::path(delete, path = "/api/v1/tenants/{tid}/reference-lists/{id}/entries/{entry_id}",
    params(("tid" = Uuid, Path), ("id" = Uuid, Path), ("entry_id" = i64, Path)), responses((status = 204)),
    tag = "reference-lists")]
pub async fn tenant_delete_entry(
    State(s): State<AppState>,
    c: Caller,
    Path((tid, id, entry_id)): Path<(Uuid, Uuid, i64)>,
) -> AppResult<StatusCode> {
    let scope = tenant_list_scope(&c, tid, Access::Write)?;
    do_delete_entry(&s, &c, scope, id, entry_id).await
}
