//! Authentication & authorisation.
//!
//! ## Model
//!
//! * **Users** carry a JWT issued by core-api (HS256, shared `JWT_SECRET`). The claims contain the
//!   user's tenant (`tid`), tenant role (`trole`), platform-admin flag (`padmin`) and a map of
//!   project → role (`prj`). Every service verifies the token **itself**, so there is no network
//!   call per request (see docs/technical/architecture.md §5).
//! * **Services** call each other with `Authorization: Bearer ${INTERNAL_API_TOKEN}` plus
//!   `X-Tenant-Id`, `X-Project-Id` and `X-Actor` headers. These are trusted because only services
//!   on the internal network know the token, and the gateway never routes `/v1/...` paths.
//!
//! ## Why extractors instead of filters/annotations
//!
//! In Spring you would write a `OncePerRequestFilter` plus `@PreAuthorize("hasRole(...)")`. In axum a
//! handler declares what it needs as parameters: `async fn handler(caller: Caller, ...)`.
//! [`Caller`] implements `FromRequestParts`, so axum runs our authentication code before the handler.
//! If that code fails, the handler never runs and the client gets a 401 problem+json. This is checked at
//! compile time: a handler without a `Caller` parameter has no identity to use, so a missing
//! authorisation check is visible in its signature.
//!
//! Project-level checks are explicit calls (`caller.require_project_role(pid, ProjectRole::Analyst)`)
//! because the project id comes from the path, and the check needs to be async (a tenant admin's
//! access requires a DB lookup; see [`ProjectDirectory`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::http::HeaderMap;
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Secret;
use crate::error::{AppError, AppResult};
use crate::ids::{ProjectId, TenantId, UserId};

pub const HEADER_TENANT: &str = "x-tenant-id";
pub const HEADER_PROJECT: &str = "x-project-id";
pub const HEADER_ACTOR: &str = "x-actor";

/// Role of a user inside one project. The derive order defines the privilege order:
/// `Viewer < Analyst < Approver < ProjectAdmin`, so `role >= ProjectRole::Analyst` just works.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Viewer,
    Analyst,
    Approver,
    ProjectAdmin,
}

impl ProjectRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Analyst => "analyst",
            Self::Approver => "approver",
            Self::ProjectAdmin => "project_admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Self::Viewer),
            "analyst" => Some(Self::Analyst),
            "approver" => Some(Self::Approver),
            "project_admin" => Some(Self::ProjectAdmin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TenantRole {
    TenantAdmin,
    Member,
}

/// JWT access-token claims (architecture.md §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    /// Tenant of the user; `None` only for platform admins.
    pub tid: Option<Uuid>,
    pub trole: TenantRole,
    /// Platform admin flag.
    pub padmin: bool,
    /// Project id → role, for projects of `tid` the user is a member of.
    #[serde(default)]
    pub prj: HashMap<Uuid, ProjectRole>,
    pub exp: i64,
    pub iat: i64,
    pub jti: Uuid,
}

impl Claims {
    /// Builds claims valid for `ttl` from now.
    pub fn new(
        user_id: Uuid,
        tenant_id: Option<Uuid>,
        trole: TenantRole,
        padmin: bool,
        prj: HashMap<Uuid, ProjectRole>,
        ttl: Duration,
    ) -> Self {
        let now = Utc::now().timestamp();
        Self {
            sub: user_id,
            tid: tenant_id,
            trole,
            padmin,
            prj,
            exp: now + ttl.as_secs() as i64,
            iat: now,
            jti: Uuid::new_v4(),
        }
    }
}

/// HS256 signing/verification keys.
#[derive(Clone)]
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JwtKeys(***)")
    }
}

impl JwtKeys {
    pub fn new(secret: &Secret) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 30;
        validation.validate_exp = true;
        validation.set_required_spec_claims(&["exp", "sub", "iat"]);
        Self {
            encoding: EncodingKey::from_secret(secret.expose().as_bytes()),
            decoding: DecodingKey::from_secret(secret.expose().as_bytes()),
            validation,
        }
    }

    pub fn issue(&self, claims: &Claims) -> AppResult<String> {
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), claims, &self.encoding)
            .map_err(|e| AppError::internal(format!("jwt encode: {e}")))
    }

    pub fn verify(&self, token: &str) -> AppResult<Claims> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map(|d| d.claims)
            .map_err(|e| AppError::Unauthorized(format!("invalid token: {e}")))
    }
}

/// Resolves whether a project belongs to a tenant.
///
/// This is a trait (Java: interface) because the answer comes from the database in production but
/// from a fixed map in unit tests. [`PgProjectDirectory`] is the production implementation.
#[async_trait]
pub trait ProjectDirectory: Send + Sync + 'static {
    /// `true` if `project_id` exists and belongs to `tenant_id`.
    async fn project_in_tenant(&self, project_id: ProjectId, tenant_id: TenantId) -> AppResult<bool>;
}

/// Reads `core.projects` under the claimed tenant (RLS guarantees cross-tenant rows are invisible).
/// Results are cached for 60 s; projects never move between tenants.
#[derive(Debug, Clone)]
pub struct PgProjectDirectory {
    pool: sqlx::PgPool,
    cache: Arc<tokio::sync::RwLock<ProjectCache>>,
}

/// (project, tenant) → (belongs, cached_at)
type ProjectCache = HashMap<(ProjectId, TenantId), (bool, std::time::Instant)>;

impl PgProjectDirectory {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            cache: Arc::default(),
        }
    }
}

#[async_trait]
impl ProjectDirectory for PgProjectDirectory {
    async fn project_in_tenant(&self, project_id: ProjectId, tenant_id: TenantId) -> AppResult<bool> {
        const TTL: Duration = Duration::from_secs(60);
        if let Some((hit, at)) = self.cache.read().await.get(&(project_id, tenant_id)) {
            if at.elapsed() < TTL {
                return Ok(*hit);
            }
        }
        let mut tx = crate::db::TenantTx::begin(&self.pool, tenant_id).await?;
        let found: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM core.projects WHERE id = $1 AND status = 'active'")
                .bind(project_id.as_uuid())
                .fetch_optional(&mut **tx)
                .await?;
        tx.commit().await?;
        let hit = found.is_some();
        self.cache
            .write()
            .await
            .insert((project_id, tenant_id), (hit, std::time::Instant::now()));
        Ok(hit)
    }
}

/// Shared auth dependencies, placed in each service's axum state.
/// Services implement `FromRef<AppState> for AuthState` so the extractors can find it.
#[derive(Clone)]
pub struct AuthState {
    pub jwt: Arc<JwtKeys>,
    internal_token: Arc<Secret>,
    pub projects: Arc<dyn ProjectDirectory>,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState").finish_non_exhaustive()
    }
}

impl AuthState {
    pub fn new(jwt_secret: &Secret, internal_token: &Secret, projects: Arc<dyn ProjectDirectory>) -> Self {
        Self {
            jwt: Arc::new(JwtKeys::new(jwt_secret)),
            internal_token: Arc::new(internal_token.clone()),
            projects,
        }
    }

    fn is_internal_token(&self, presented: &str) -> bool {
        constant_time_eq(presented.as_bytes(), self.internal_token.expose().as_bytes())
    }
}

/// Constant-time comparison (length is not secret).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn bearer(headers: &HeaderMap) -> AppResult<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("missing Authorization header".into()))?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::Unauthorized("expected a Bearer token".into()))
}

/// `X-Actor` as free text (trimmed, max 200 chars, visible ASCII only). Never rejects a request:
/// an unusable value is simply dropped.
fn header_actor(headers: &HeaderMap) -> Option<String> {
    headers
        .get(HEADER_ACTOR)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 200 && s.chars().all(|c| c.is_ascii_graphic()))
        .map(String::from)
}

fn header_uuid(headers: &HeaderMap, name: &str) -> AppResult<Option<Uuid>> {
    match headers.get(name) {
        None => Ok(None),
        Some(v) => {
            let s = v
                .to_str()
                .map_err(|_| AppError::BadRequest(format!("invalid {name} header")))?;
            Uuid::parse_str(s.trim())
                .map(Some)
                .map_err(|_| AppError::BadRequest(format!("invalid {name} header")))
        }
    }
}

/// An authenticated end user (JWT). Use when an endpoint is **only** for users.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub claims: Claims,
}

impl AuthUser {
    pub fn user_id(&self) -> UserId {
        UserId(self.claims.sub)
    }
    pub fn tenant_id(&self) -> Option<TenantId> {
        self.claims.tid.map(TenantId)
    }
    pub fn is_platform_admin(&self) -> bool {
        self.claims.padmin
    }
    pub fn is_tenant_admin_of(&self, tenant: TenantId) -> bool {
        self.claims.trole == TenantRole::TenantAdmin && self.claims.tid == Some(tenant.0)
    }
    pub fn project_role(&self, project: ProjectId) -> Option<ProjectRole> {
        self.claims.prj.get(&project.0).copied()
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    AuthState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        let token = bearer(&parts.headers)?;
        if auth.is_internal_token(token) {
            return Err(AppError::Forbidden("endpoint requires a user token".into()));
        }
        Ok(Self {
            claims: auth.jwt.verify(token)?,
        })
    }
}

/// Who is calling: a user (JWT) or another service (internal token).
#[derive(Debug, Clone)]
pub enum CallerKind {
    User(AuthUser),
    Service {
        tenant_id: TenantId,
        project_id: Option<ProjectId>,
        /// Raw `X-Actor` value: either the UUID of the end user on whose behalf the service acts,
        /// or a service principal such as `ingest-service:poller` / `llm-service`.
        /// Use [`Caller::actor_user_id`] to get it as a user id (only when it is a UUID).
        actor: Option<String>,
    },
}

/// Extractor accepting either a user JWT or the internal service token.
#[derive(Clone)]
pub struct Caller {
    pub kind: CallerKind,
    projects: Arc<dyn ProjectDirectory>,
}

impl std::fmt::Debug for Caller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Caller")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl<S> FromRequestParts<S> for Caller
where
    AuthState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        let token = bearer(&parts.headers)?;
        let kind = if auth.is_internal_token(token) {
            let tenant = header_uuid(&parts.headers, HEADER_TENANT)?
                .ok_or_else(|| AppError::BadRequest("internal call requires X-Tenant-Id".into()))?;
            CallerKind::Service {
                tenant_id: TenantId(tenant),
                project_id: header_uuid(&parts.headers, HEADER_PROJECT)?.map(ProjectId),
                actor: header_actor(&parts.headers),
            }
        } else {
            CallerKind::User(AuthUser {
                claims: auth.jwt.verify(token)?,
            })
        };
        Ok(Self {
            kind,
            projects: auth.projects.clone(),
        })
    }
}

impl Caller {
    /// Builds a caller directly (tests, background jobs).
    pub fn new(kind: CallerKind, projects: Arc<dyn ProjectDirectory>) -> Self {
        Self { kind, projects }
    }

    pub fn is_service(&self) -> bool {
        matches!(self.kind, CallerKind::Service { .. })
    }

    pub fn user(&self) -> Option<&AuthUser> {
        match &self.kind {
            CallerKind::User(u) => Some(u),
            CallerKind::Service { .. } => None,
        }
    }

    /// The user responsible for the action: the JWT subject, or `X-Actor` for service calls **when
    /// it is a UUID**. Service principals (`ingest-service:poller`) yield `None`, so they are never
    /// written into `*_by uuid` columns.
    pub fn actor_user_id(&self) -> Option<UserId> {
        match &self.kind {
            CallerKind::User(u) => Some(u.user_id()),
            CallerKind::Service { actor, .. } => {
                actor.as_deref().and_then(|a| Uuid::parse_str(a).ok()).map(UserId)
            }
        }
    }

    /// Raw `X-Actor` of a service call (UUID or service principal name).
    pub fn raw_actor(&self) -> Option<&str> {
        match &self.kind {
            CallerKind::User(_) => None,
            CallerKind::Service { actor, .. } => actor.as_deref(),
        }
    }

    /// `(actor_type, actor_id)` for `core.audit_log`: `user` when the actor is a user id,
    /// otherwise `service` with the raw principal name (or `None` when no `X-Actor` was sent).
    pub fn audit_actor(&self) -> (&'static str, Option<String>) {
        match &self.kind {
            CallerKind::User(u) => ("user", Some(u.claims.sub.to_string())),
            CallerKind::Service { actor, .. } => match self.actor_user_id() {
                Some(uid) => ("user", Some(uid.to_string())),
                None => ("service", actor.clone()),
            },
        }
    }

    /// Authorises access to a project with at least `min_role` and returns the project's tenant.
    ///
    /// * Service callers: allowed for any role, provided `X-Project-Id` (if sent) matches.
    /// * Project members: role from the `prj` claim must be `>= min_role`.
    /// * Tenant admins: implicitly `ProjectAdmin` on every project of their tenant (verified in DB).
    /// * Platform admins get **no** implicit access to tenant data (privacy by default).
    pub async fn require_project_role(
        &self,
        project: ProjectId,
        min_role: ProjectRole,
    ) -> AppResult<TenantId> {
        match &self.kind {
            CallerKind::Service {
                tenant_id,
                project_id,
                ..
            } => {
                if let Some(p) = project_id {
                    if *p != project {
                        return Err(AppError::Forbidden(
                            "X-Project-Id does not match the requested project".into(),
                        ));
                    }
                }
                Ok(*tenant_id)
            }
            CallerKind::User(user) => {
                let tenant = user
                    .tenant_id()
                    .ok_or_else(|| AppError::Forbidden("no access to this project".into()))?;
                if let Some(role) = user.project_role(project) {
                    return if role >= min_role {
                        Ok(tenant)
                    } else {
                        Err(AppError::Forbidden(format!(
                            "requires project role `{}`",
                            min_role.as_str()
                        )))
                    };
                }
                if user.claims.trole == TenantRole::TenantAdmin
                    && self.projects.project_in_tenant(project, tenant).await?
                {
                    return Ok(tenant);
                }
                // 404 rather than 403: do not reveal whether the project exists.
                Err(AppError::NotFound("project not found".into()))
            }
        }
    }

    /// Requires the caller to be a tenant admin of `tenant` (or a service acting for it).
    pub fn require_tenant_admin(&self, tenant: TenantId) -> AppResult<()> {
        match &self.kind {
            CallerKind::Service { tenant_id, .. } if *tenant_id == tenant => Ok(()),
            CallerKind::User(u) if u.is_tenant_admin_of(tenant) => Ok(()),
            _ => Err(AppError::Forbidden("requires tenant admin".into())),
        }
    }

    /// Requires membership of `tenant` (any role) — e.g. reading the tenant regulation library.
    ///
    /// Platform admins deliberately do **not** pass: privacy by default, a platform admin manages
    /// tenants and users but has no access to tenant data (projects, events, regulations). Use
    /// [`Caller::require_platform_admin`] for platform-level endpoints instead.
    pub fn require_tenant_member(&self, tenant: TenantId) -> AppResult<()> {
        match &self.kind {
            CallerKind::Service { tenant_id, .. } if *tenant_id == tenant => Ok(()),
            CallerKind::User(u) if u.tenant_id() == Some(tenant) => Ok(()),
            _ => Err(AppError::NotFound("tenant not found".into())),
        }
    }

    pub fn require_platform_admin(&self) -> AppResult<()> {
        match &self.kind {
            CallerKind::User(u) if u.is_platform_admin() => Ok(()),
            _ => Err(AppError::Forbidden("requires platform admin".into())),
        }
    }

    pub fn require_service(&self) -> AppResult<TenantId> {
        match &self.kind {
            CallerKind::Service { tenant_id, .. } => Ok(*tenant_id),
            CallerKind::User(_) => Err(AppError::Forbidden("internal endpoint".into())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use axum::http::Request;

    struct FixedDirectory(Vec<(ProjectId, TenantId)>);

    #[async_trait]
    impl ProjectDirectory for FixedDirectory {
        async fn project_in_tenant(&self, p: ProjectId, t: TenantId) -> AppResult<bool> {
            Ok(self.0.contains(&(p, t)))
        }
    }

    #[derive(Clone)]
    struct TestState(AuthState);
    impl FromRef<TestState> for AuthState {
        fn from_ref(s: &TestState) -> AuthState {
            s.0.clone()
        }
    }

    fn state(dir: Vec<(ProjectId, TenantId)>) -> TestState {
        TestState(AuthState::new(
            &Secret::new("test-jwt-secret-test-jwt-secret!!"),
            &Secret::new("internal-token"),
            Arc::new(FixedDirectory(dir)),
        ))
    }

    fn parts(headers: &[(&str, &str)]) -> Parts {
        let mut b = Request::builder().uri("/x");
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(()).unwrap().into_parts().0
    }

    fn user_token(
        st: &TestState,
        trole: TenantRole,
        tenant: Uuid,
        prj: HashMap<Uuid, ProjectRole>,
    ) -> String {
        let c = Claims::new(
            Uuid::new_v4(),
            Some(tenant),
            trole,
            false,
            prj,
            Duration::from_secs(60),
        );
        st.0.jwt.issue(&c).unwrap()
    }

    #[test]
    fn roles_are_ordered() {
        assert!(ProjectRole::ProjectAdmin > ProjectRole::Approver);
        assert!(ProjectRole::Approver > ProjectRole::Analyst);
        assert!(ProjectRole::Analyst > ProjectRole::Viewer);
        assert_eq!(ProjectRole::parse("approver"), Some(ProjectRole::Approver));
    }

    #[test]
    fn jwt_roundtrip_and_tamper_detection() {
        let keys = JwtKeys::new(&Secret::new("a-secret"));
        let c = Claims::new(
            Uuid::new_v4(),
            None,
            TenantRole::Member,
            true,
            HashMap::new(),
            Duration::from_secs(60),
        );
        let token = keys.issue(&c).unwrap();
        assert_eq!(keys.verify(&token).unwrap(), c);
        let other = JwtKeys::new(&Secret::new("another-secret"));
        assert!(matches!(other.verify(&token), Err(AppError::Unauthorized(_))));
    }

    #[test]
    fn expired_jwt_is_rejected() {
        let keys = JwtKeys::new(&Secret::new("a-secret"));
        let mut c = Claims::new(
            Uuid::new_v4(),
            None,
            TenantRole::Member,
            false,
            HashMap::new(),
            Duration::from_secs(1),
        );
        c.exp = Utc::now().timestamp() - 3600;
        let token = keys.issue(&c).unwrap();
        assert!(keys.verify(&token).is_err());
    }

    #[tokio::test]
    async fn member_role_checks() {
        let st = state(vec![]);
        let (tenant, project) = (Uuid::new_v4(), Uuid::new_v4());
        let token = user_token(
            &st,
            TenantRole::Member,
            tenant,
            HashMap::from([(project, ProjectRole::Analyst)]),
        );
        let mut p = parts(&[("authorization", &format!("Bearer {token}"))]);
        let caller = Caller::from_request_parts(&mut p, &st).await.unwrap();

        assert_eq!(
            caller
                .require_project_role(ProjectId(project), ProjectRole::Viewer)
                .await
                .unwrap(),
            TenantId(tenant)
        );
        assert!(caller
            .require_project_role(ProjectId(project), ProjectRole::Analyst)
            .await
            .is_ok());
        assert!(matches!(
            caller
                .require_project_role(ProjectId(project), ProjectRole::Approver)
                .await,
            Err(AppError::Forbidden(_))
        ));
        // Unknown project → 404, not 403
        assert!(matches!(
            caller
                .require_project_role(ProjectId(Uuid::new_v4()), ProjectRole::Viewer)
                .await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn tenant_admin_has_implicit_access_only_within_tenant() {
        let (tenant, project, foreign) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let st = state(vec![(ProjectId(project), TenantId(tenant))]);
        let token = user_token(&st, TenantRole::TenantAdmin, tenant, HashMap::new());
        let mut p = parts(&[("authorization", &format!("Bearer {token}"))]);
        let caller = Caller::from_request_parts(&mut p, &st).await.unwrap();

        assert!(caller
            .require_project_role(ProjectId(project), ProjectRole::ProjectAdmin)
            .await
            .is_ok());
        assert!(caller
            .require_project_role(ProjectId(foreign), ProjectRole::Viewer)
            .await
            .is_err());
        assert!(caller.require_tenant_admin(TenantId(tenant)).is_ok());
        assert!(caller.require_tenant_admin(TenantId(Uuid::new_v4())).is_err());
    }

    #[tokio::test]
    async fn internal_token_requires_tenant_header_and_matching_project() {
        let st = state(vec![]);
        let (tenant, project) = (Uuid::new_v4(), Uuid::new_v4());

        let mut p = parts(&[("authorization", "Bearer internal-token")]);
        assert!(matches!(
            Caller::from_request_parts(&mut p, &st).await,
            Err(AppError::BadRequest(_))
        ));

        let t = tenant.to_string();
        let pr = project.to_string();
        let mut p = parts(&[
            ("authorization", "Bearer internal-token"),
            (HEADER_TENANT, &t),
            (HEADER_PROJECT, &pr),
        ]);
        let caller = Caller::from_request_parts(&mut p, &st).await.unwrap();
        assert!(caller.is_service());
        assert_eq!(caller.require_service().unwrap(), TenantId(tenant));
        assert_eq!(
            caller
                .require_project_role(ProjectId(project), ProjectRole::ProjectAdmin)
                .await
                .unwrap(),
            TenantId(tenant)
        );
        assert!(caller
            .require_project_role(ProjectId(Uuid::new_v4()), ProjectRole::Viewer)
            .await
            .is_err());
        assert_eq!(caller.audit_actor(), ("service", None));
        assert_eq!(caller.actor_user_id(), None);
    }

    #[tokio::test]
    async fn wrong_token_is_unauthorized_and_user_only_endpoint_rejects_service() {
        let st = state(vec![]);
        let mut p = parts(&[("authorization", "Bearer internal-tokeX")]);
        assert!(matches!(
            Caller::from_request_parts(&mut p, &st).await,
            Err(AppError::Unauthorized(_))
        ));

        let mut p = parts(&[("authorization", "Bearer internal-token")]);
        assert!(matches!(
            AuthUser::from_request_parts(&mut p, &st).await,
            Err(AppError::Forbidden(_))
        ));

        let mut p = parts(&[]);
        assert!(matches!(
            Caller::from_request_parts(&mut p, &st).await,
            Err(AppError::Unauthorized(_))
        ));
    }

    #[tokio::test]
    async fn x_actor_accepts_uuid_and_service_principals() {
        let st = state(vec![]);
        let tenant = Uuid::new_v4().to_string();
        let user = Uuid::new_v4();

        let u = user.to_string();
        let mut p = parts(&[
            ("authorization", "Bearer internal-token"),
            (HEADER_TENANT, &tenant),
            (HEADER_ACTOR, &u),
        ]);
        let caller = Caller::from_request_parts(&mut p, &st).await.unwrap();
        assert_eq!(caller.actor_user_id(), Some(UserId(user)));
        assert_eq!(caller.audit_actor(), ("user", Some(u.clone())));

        let mut p = parts(&[
            ("authorization", "Bearer internal-token"),
            (HEADER_TENANT, &tenant),
            (HEADER_ACTOR, "ingest-service:poller"),
        ]);
        let caller = Caller::from_request_parts(&mut p, &st).await.unwrap();
        assert_eq!(caller.actor_user_id(), None);
        assert_eq!(caller.raw_actor(), Some("ingest-service:poller"));
        assert_eq!(
            caller.audit_actor(),
            ("service", Some("ingest-service:poller".to_string()))
        );
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
