// Types mirroring docs/technical/api-contract.md (gateway /api/v1). Keep in sync with the contract.
// Rule DSL types live in shared/rules/dsl.ts.

import type { RuleDefinition, RuleEnvelope, RuleKind, RuleStatus } from '../rules/dsl'

export type Uuid = string
export type IsoDateTime = string

export interface Page<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

export interface PageQuery {
  page?: number
  page_size?: number
}

// ---------------------------------------------------------------- identity & tenancy
export type ProjectRole = 'project_admin' | 'approver' | 'analyst' | 'viewer'
export type TenantRole = 'tenant_admin' | 'member'

export interface UserSummary {
  id: Uuid
  email: string
  full_name: string
  tenant_id: Uuid | null
  tenant_role: TenantRole
  is_platform_admin: boolean
  is_active?: boolean
  last_login_at?: IsoDateTime | null
}

export interface ProjectMembership {
  id: Uuid
  slug: string
  name: string
  stage: ProjectStage
  status?: 'active' | 'archived'
  role: ProjectRole
}

export interface Me {
  user: UserSummary
  tenant: Tenant | null
  projects: ProjectMembership[]
}

export interface LoginResponse {
  access_token: string
  token_type: 'Bearer'
  expires_in: number
  refresh_token: string
  user: UserSummary
}

export interface Tenant {
  id: Uuid
  slug: string
  name: string
  status: 'active' | 'suspended'
  users?: number
  created_at?: IsoDateTime
}

export type ProjectStage = 'pre_payment' | 'post_payment' | 'returns' | 'promo' | 'account_security' | 'payout' | 'custom'

export interface MlConfig {
  supervised: { algorithm: string, params: Record<string, unknown> }
  unsupervised: {
    anomaly_algorithm: string
    anomaly_params: Record<string, unknown>
    clustering_algorithm: string
    clustering_params: Record<string, unknown>
  }
  features: { include: string[], exclude: string[], extra_source_fields: string[] }
}

export interface LlmConfig {
  chat_model: string | null
  temperature: number
  language: 'id' | 'en'
  system_prompt_extra: string
}

export type LinkKind = 'email' | 'phone' | 'device' | 'ip' | 'card' | 'bank_account' | 'address' | 'ref_transaction' | 'api_client'

export interface GraphConfig {
  link_kinds: LinkKind[]
  include_similar: boolean
  max_depth: number
  supernode_degree_cap: number
  similarity_threshold: number
}

export interface Project {
  id: Uuid
  tenant_id: Uuid
  slug: string
  name: string
  description: string | null
  stage: ProjectStage
  business_context: string | null
  timezone: string
  currency: string
  status: 'active' | 'archived'
  ml_config: MlConfig
  llm_config: LlmConfig
  graph_config: GraphConfig
  created_at: IsoDateTime
  summary?: { events_30d: number, decisions_30d?: number, declines_30d?: number, open_cases: number, active_rules: number | null, active_models: number | null }
}

/** GET /projects list item (summary only — full config via GET /projects/{pid}). */
export interface ProjectListItem {
  id: Uuid
  slug: string
  name: string
  description: string | null
  stage: ProjectStage
  status: 'active' | 'archived'
  role?: ProjectRole
  currency?: string
  timezone?: string
  created_at?: IsoDateTime
  business_context?: string | null
  ml_config?: MlConfig
}

export interface ProjectCreate {
  slug: string
  name: string
  description?: string
  stage: ProjectStage
  business_context?: string
  timezone?: string
  currency?: string
  template?: ProjectStage | 'none'
  ml_config?: MlConfig
  llm_config?: LlmConfig
  graph_config?: GraphConfig
}

export interface ProjectMember {
  user_id: Uuid
  email: string
  full_name: string
  role: ProjectRole
  is_active?: boolean
  created_at?: IsoDateTime
}

export type EngineCombination = 'noisy_or' | 'weighted_average'

export interface ProjectSettings {
  decision_thresholds: { review: number, decline: number }
  /** How engine scores combine into final_score (default noisy_or). */
  engine_combination: EngineCombination
  engine_weights: { rules: number, supervised: number, unsupervised: number, graph: number }
  graph_scores: { fraud_distance_scores: Record<string, number>, shared_fraud_entity_score: number }
  timeouts: { graph_ms: number, ml_ms: number, rules_ms: number }
  cases: { auto_create_on: Decision[] } // one open case per customer is always enforced
  rules_unavailable_decision: Decision
}

// ---------------------------------------------------------------- events & decisions
export type Decision = 'approve' | 'review' | 'decline'
export type Engine = 'rules' | 'supervised' | 'unsupervised' | 'graph'
export type RuleOutcome = 'match' | 'no_match' | 'trapped'
export type FraudType = 'carding' | 'account_takeover' | 'bank_account_takeover' | 'system_breach' | 'promo_abuse' | 'refund_abuse' | 'money_mule' | 'other'
export type Typology = FraudType

/** `contribution` is an attribution of final_score: contributions of all reasons sum to it. */
export interface Reason {
  code: string
  engine: Engine
  contribution: number
  message: string
}

export interface RuleResult {
  rule_id: Uuid
  rule_code: string
  version: number
  ruleset_code?: string | null
  kind: RuleKind
  outcome: RuleOutcome
  contribution: number
  shadow: boolean
  action: string
  trapped_reason: string | null
  trace: Record<string, unknown>
  duration_us?: number
}

export interface MlOutput {
  fraud_probability?: number | null
  anomaly_score?: number | null
  cluster_id?: number | null
  cluster_fraud_rate?: number | null
  supervised_model?: { id: Uuid, version: number, algorithm: string } | null
  unsupervised_model?: { id: Uuid, version: number } | null
}

export interface GraphMetrics {
  shared_with_fraud_kinds?: string[]
  distance_to_fraud: number | null
  fraud_neighbors_1?: number
  fraud_neighbors_2?: number
  component_size?: number
  shared_entity_count?: number
  degree?: number
  community_fraud_rate?: number | null
}

export interface DecisionOut {
  event_id: Uuid
  external_id: string
  project_id: Uuid
  decision: Decision
  final_score: number
  engine_scores: Partial<Record<Engine, number | null>>
  reasons: Reason[]
  rule_results: RuleResult[]
  ml: MlOutput
  graph: GraphMetrics
  degraded: Engine[]
  case_id: Uuid | null
  latency_ms: number
  persisted: boolean
  created_at?: IsoDateTime
}

export interface EventSummary {
  load_only?: boolean
  received_at?: IsoDateTime
  id: Uuid
  external_id: string
  event_type: string
  occurred_at: IsoDateTime
  customer_id: Uuid
  customer_external_id: string
  amount: number | null
  currency: string | null
  channel: string | null
  decision: Decision | null
  final_score: number | null
  data_source_id: Uuid
}

export interface Label {
  id: Uuid
  subject_type: 'event' | 'customer'
  subject_id: Uuid
  label: 'fraud' | 'legit'
  fraud_type: FraudType | null
  source: 'analyst' | 'chargeback' | 'customer_report' | 'dataset' | 'system'
  notes: string | null
  created_by: Uuid | null
  created_at: IsoDateTime
}

/** Canonical event row (events table columns). */
export interface CanonicalEvent {
  id: Uuid
  external_id: string
  event_type: string
  occurred_at: IsoDateTime
  customer_id: Uuid
  amount: number | null
  currency: string | null
  channel: string | null
  data_source_id: Uuid
  [field: string]: unknown
}

/** GET /projects/{pid}/events/{id} */
export interface EventDetail {
  event: CanonicalEvent
  source: Record<string, unknown> | null
  features: Record<string, unknown> | null
  customer: Customer | null
  decision: DecisionOut | null
  labels: Label[]
  case: { id: Uuid, status: CaseStatus, priority: number } | null
}

export interface Customer {
  id: Uuid
  external_id: string
  full_name: string | null
  email: string | null
  phone: string | null
  risk_label: 'fraud' | 'legit' | 'unknown'
  status: string
  segment?: string | null
  kyc_level?: number | null
  registered_at?: IsoDateTime | null
  attributes?: Record<string, unknown>
  stats?: { events_30d: number, declines_30d: number, avg_score_30d: number | null, open_case_id?: Uuid | null }
}

export type CaseStatus = 'open' | 'in_review' | 'resolved_fraud' | 'resolved_legit'

export interface CaseNote {
  at: IsoDateTime
  by: string
  text: string
}

/** Case list item (GET /cases). Assignee names are resolved via GET /members. */
export interface CaseSummary {
  id: Uuid
  status: CaseStatus
  priority: number
  typologies: Typology[]
  assigned_to: Uuid | null
  customer_id: Uuid
  customer_external_id: string
  event_id: Uuid | null
  event_count: number
  decision: Decision | null
  final_score: number | null
  risk_label: 'fraud' | 'legit' | 'unknown'
  created_at: IsoDateTime
  updated_at: IsoDateTime
  resolved_at: IsoDateTime | null
}

/** Case record inside GET /cases/{id}. */
export interface Case {
  id: Uuid
  customer_id: Uuid
  event_id: Uuid | null
  decision_id: Uuid | null
  status: CaseStatus
  priority: number
  typologies: Typology[]
  assigned_to: Uuid | null
  notes: CaseNote[]
  event_ids: Uuid[]
  created_at: IsoDateTime
  updated_at: IsoDateTime
  resolved_at: IsoDateTime | null
}

/** GET /projects/{pid}/cases/{id} */
export interface CaseDetail {
  case: Case
  customer: Customer | null
  event: CanonicalEvent | null
  decision: DecisionOut | null
  graph: GraphMetrics | null
}

// ---------------------------------------------------------------- analytics & audit
export interface AnalyticsOverview {
  totals: { events: number, approve: number, review: number, decline: number }
  by_event_type: { event_type: string, count: number }[]
  by_label_fraud_type: { fraud_type: FraudType, count: number }[]
  daily: { date: string, events: number, review: number, decline: number, avg_score: number | null }[]
  /** bucket = lower bound of a 10-point score bucket (0, 10, … 90) */
  score_histogram: { bucket: number | string, count: number }[]
  engine_avg: Partial<Record<Engine, number | null>>
  from?: string
  to?: string
  open_cases: number
  degraded_rate: number
}

export interface DriftRow {
  feature: string
  psi: number
  recent_mean: number
  baseline_mean: number
  status: 'stable' | 'moderate' | 'significant'
  recent_n?: number
  baseline_n?: number
}

export interface DriftResponse {
  recent_window: { from: string, to: string }
  baseline_window: { from: string, to: string }
  items: DriftRow[]
}

export interface AuditEntry {
  id: number
  occurred_at: IsoDateTime
  actor_type: 'user' | 'service' | 'system'
  actor_id: string | null
  action: string
  subject_type: string | null
  subject_id: string | null
  before: unknown
  after: unknown
  metadata: Record<string, unknown>
  request_id: string | null
}

// ---------------------------------------------------------------- rules
export interface RuleStats {
  evaluated: number
  matched: number
  trapped: number
}

/** Which versions currently serve traffic (live) and run in shadow. */
export interface Serving {
  live_version: number | null
  shadow_version: number | null
}

export interface Rule {
  id: Uuid
  code: string
  name: string
  description: string | null
  kind: RuleKind
  typologies: Typology[]
  event_types: string[]
  status: RuleStatus
  current_version: number
  submitted_by: Uuid | null
  created_by: Uuid | null
  created_at: IsoDateTime
  updated_at: IsoDateTime
  envelope: RuleEnvelope // current version
  serving?: Serving
  stats_7d?: RuleStats
}

export interface RuleVersion {
  version: number
  envelope: RuleEnvelope
  change_note: string | null
  created_by: Uuid | null
  created_at: IsoDateTime
}

export interface Approval {
  id: Uuid
  subject_version: number | null
  requested_by: Uuid | null
  requested_at: IsoDateTime
  decided_by: Uuid | null
  decided_at: IsoDateTime | null
  decision: 'approved' | 'rejected' | null
  target_status: string | null
  comment: string | null
}

export interface RuleDetail extends Rule {
  versions: RuleVersion[]
  approvals?: Approval[]
}

export interface ValidationError {
  path: string
  message: string
}

export interface ValidationResult {
  valid: boolean
  errors: ValidationError[]
  referenced_fields: string[]
  referenced_lists: string[]
}

export interface RuleTestResult {
  outcome: RuleOutcome
  contribution: number
  trapped_reason: string | null
  trace: Record<string, unknown>
}

export interface BacktestResult {
  evaluated: number
  matched: number
  trapped: number
  hit_rate: number
  labeled_fraud_matched: number
  labeled_legit_matched: number
  labeled_fraud_total?: number
  truncated?: boolean
  precision: number | null
  recall: number | null
  sample_matches: Uuid[]
  by_day: { date: string, matched: number, evaluated: number }[]
  score_histogram?: { bucket: number | string, count: number }[]
  decision_distribution?: Partial<Record<Decision, number>>
}

export interface RulePerformanceResponse {
  since_days: number
  items: RulePerformance[]
}

export interface RulePerformance {
  rule_id: Uuid
  code: string
  status: RuleStatus
  evaluated: number
  matched: number
  trapped: number
  hit_rate: number
  precision: number | null
  last_hit_at: IsoDateTime | null
}

export type RulesetAggregation = 'sum' | 'max' | 'probabilistic_or' | 'weighted_average'

export interface RulesetMember {
  rule_id: Uuid
  rule_code?: string
  rule_name?: string
  rule_status?: RuleStatus
  weight: number
  pinned_version: number | null
  position?: number
}

export interface Ruleset {
  id: Uuid
  code: string
  name: string
  description: string | null
  event_types: string[]
  typologies: Typology[]
  aggregation: RulesetAggregation
  max_score: number
  version: number
  status: RuleStatus
  serving?: Serving
  rules: RulesetMember[]
  created_at: IsoDateTime
  updated_at: IsoDateTime
}

export type ListType = 'blacklist' | 'whitelist' | 'watchlist' | 'lookup'

export interface ReferenceList {
  id: Uuid
  name: string
  description: string | null
  list_type: ListType
  key_kind: string
  columns: { name: string, type: string }[]
  scope: 'project' | 'tenant'
  entry_count?: number
  created_at: IsoDateTime
}

export interface ReferenceEntry {
  id: number
  key: string
  attributes: Record<string, unknown>
  valid_from: IsoDateTime
  valid_until: IsoDateTime | null
  reason: string | null
  created_at: IsoDateTime
}

export interface FormulaResult {
  value?: number | null
  trapped: boolean
  reason?: string
  name?: string
  params?: string[]
}

export type ProposalType = 'new_rule' | 'modify_rule' | 'retire_rule' | 'tune_threshold'
export type ProposalStatus = 'pending' | 'approved' | 'rejected' | 'applied'

export interface Citation {
  regulation_id: Uuid
  chunk_id?: string
  code?: string
  version?: number
  section?: string
  excerpt: string
  score?: number
}

export interface Proposal {
  id: Uuid
  source: 'llm' | 'analyst'
  proposal_type: ProposalType
  target_rule_id: Uuid | null
  target_rule_code?: string | null
  definition: RuleEnvelope | null
  rationale: string
  citations: Citation[]
  evidence: Record<string, unknown>
  validation: Partial<ValidationResult>
  backtest: BacktestResult | null
  report_id: Uuid | null
  llm_model: string | null
  status: ProposalStatus
  created_at: IsoDateTime
  reviewed_by: Uuid | null
  reviewed_at: IsoDateTime | null
  review_comment: string | null
  applied_rule_id: Uuid | null
}

// ---------------------------------------------------------------- graph
export interface GraphNode {
  id: string
  type: 'customer' | 'entity'
  label: string
  kind?: LinkKind
  risk_label?: 'fraud' | 'legit' | 'unknown'
  is_center?: boolean
  depth?: number
  customer_count?: number
}

export interface GraphEdge {
  id: string
  source: string
  target: string
  kind: LinkKind | 'similar'
  similarity?: number
}

export interface Neighborhood {
  nodes: GraphNode[]
  edges: GraphEdge[]
  truncated?: boolean
}

export interface FraudProximity {
  distance: number | null
  /** node sequence from the customer to the nearest fraud customer (customers and shared entities) */
  path: GraphNode[]
  truncated?: boolean
  nearest_fraud_customer_id: Uuid | null
  fraud_within: Record<string, number>
}

export interface GraphComponent {
  component_id: string
  size: number
  fraud_count: number
  fraud_rate: number
  sample_customer_ids: Uuid[]
}

export interface GraphStats {
  fraud_customers?: number
  supernode_degree_cap?: number
  customers: number
  entities: number
  links: number
  similarity_links: number
  supernodes: { kind: LinkKind, display_value: string, degree: number }[]
}

export interface GraphSearchResult {
  customers: { id: Uuid, external_id: string, risk_label: 'fraud' | 'legit' | 'unknown' }[]
  entities: { id: string, kind: LinkKind, label: string, customer_count?: number }[]
}

// ---------------------------------------------------------------- ML
export type AlgorithmKind = 'supervised' | 'anomaly' | 'clustering'

export interface JsonSchema {
  type?: string | string[]
  title?: string
  description?: string
  default?: unknown
  enum?: unknown[]
  minimum?: number
  maximum?: number
  exclusiveMinimum?: number
  exclusiveMaximum?: number
  multipleOf?: number
  minItems?: number
  maxItems?: number
  minLength?: number
  maxLength?: number
  items?: JsonSchema
  properties?: Record<string, JsonSchema>
  required?: string[]
  additionalProperties?: boolean | JsonSchema
  [k: string]: unknown
}

/** Wrapper used by several ml-service list endpoints. */
export interface ModelItems<T> {
  model_id?: string
  items: T[]
}

export interface MlAlgorithm {
  name: string
  kind: AlgorithmKind
  version: string
  display_name: string
  description: string | null
  param_schema: JsonSchema
  source: 'builtin' | 'plugin'
  status: 'available' | 'invalid' | 'disabled'
  error: string | null
  defaults?: Record<string, unknown>
}

export type ModelStatus = 'training' | 'ready' | 'pending_approval' | 'active' | 'archived' | 'failed'

export interface CurvePoint { x: number, y: number, threshold?: number }

export interface SupervisedMetrics {
  roc_auc?: number
  pr_auc?: number
  /** optional explicit curves; otherwise the PR curve is derived from `thresholds` */
  roc_curve?: CurvePoint[]
  pr_curve?: CurvePoint[]
  thresholds?: { threshold: number, precision: number, recall: number, f1: number, flagged_rate?: number }[]
  confusion_matrix?: { threshold?: number, tp: number, fp: number, tn: number, fn: number }
  calibration?: { count: number, bin_lower: number, bin_upper: number, observed_rate: number | null, mean_predicted: number | null }[]
  feature_importance?: { feature: string, importance: number }[]
  class_balance?: { fraud: number, legit: number, fraud_rate?: number }
  split?: string
  n?: number
  [k: string]: unknown
}

export interface TrainingHistory {
  train_loss?: number[]
  val_loss?: number[]
  val_pr_auc?: number[]
  best_epoch?: number
  epochs_run?: number
  [k: string]: unknown
}

export interface MlModel {
  id: Uuid
  kind: 'supervised' | 'unsupervised'
  version: number
  algorithms: Record<string, { name: string, version: string }>
  params: Record<string, unknown>
  feature_set_version: number
  feature_names: string[]
  metrics: SupervisedMetrics & Record<string, unknown>
  training_history: TrainingHistory
  status: ModelStatus
  progress: number
  trained_rows: number | null
  error: string | null
  training_started_at: IsoDateTime
  training_finished_at: IsoDateTime | null
  activated_at: IsoDateTime | null
}

export interface Cluster {
  model_id: Uuid
  cluster_id: number
  size: number
  fraud_rate: number | null
  labeled_count: number
  profile: Record<string, number>
  /** standardized mean difference vs the whole population */
  top_features: { feature: string, smd: number, cluster_mean?: number, overall_mean?: number }[]
  label: string | null
  notes: string | null
}

export interface ProjectionPoint {
  event_id: Uuid
  x: number
  y: number
  cluster_id: number | null
  anomaly_score: number
  label?: 'fraud' | 'legit' | null
}

export interface AnomalyRow {
  event_type?: string
  customer_id?: Uuid
  label?: 'fraud' | 'legit' | null
  event_id: Uuid
  external_id?: string
  anomaly_score: number
  cluster_id: number | null
  occurred_at?: IsoDateTime
  amount?: number | null
}

export interface GraphCommunity {
  community_id: number
  size: number
  fraud_count: number
  fraud_rate: number
  customer_ids: Uuid[]
}

// ---------------------------------------------------------------- LLM
export interface Regulation {
  id: Uuid
  code: string
  title: string
  doc_type: 'regulation' | 'internal_policy' | 'sop' | 'other'
  issuer: string
  version: number
  effective_date: string | null
  supersedes_id: Uuid | null
  file_name: string
  status: 'processing' | 'indexed' | 'failed' | 'superseded'
  chunk_count: number
  summary: string | null
  error: string | null
  created_at: IsoDateTime
  changes?: RegulationChange | null
}

export interface RegulationChange {
  id: Uuid
  previous_regulation_id: Uuid
  changed_sections: { section: string, change: 'added' | 'removed' | 'modified', before: string | null, after: string | null }[]
  diff_summary: string | null
}

export interface RegulationSearchHit {
  regulation_id: Uuid
  chunk_id?: string
  code: string
  version?: number
  section: string
  excerpt: string
  score: number
}

export type ReportType = 'rule_relevance' | 'fraud_situation' | 'regulation_impact' | 'recommend_rules'

export interface LlmReport {
  id: Uuid
  report_type: ReportType
  title: string
  status: 'running' | 'done' | 'failed'
  params: Record<string, unknown>
  content_md: string | null
  structured: unknown
  model: string | null
  error: string | null
  created_at: IsoDateTime
  finished_at: IsoDateTime | null
}

export interface ToolCallSummary {
  name: string
  args: Record<string, unknown>
  ok?: boolean
  result_summary?: string
}

export interface ChatMessage {
  id?: number | string
  role: 'user' | 'assistant' | 'tool' | 'system'
  content: string
  tool_calls?: ToolCallSummary[]
  citations?: Citation[]
  created_at?: IsoDateTime
}

export interface Conversation {
  id: Uuid
  title: string | null
  created_at: IsoDateTime
  messages?: ChatMessage[]
}

// ---------------------------------------------------------------- data sources
export type SourceKind = 'webhook' | 'file' | 'postgres' | 'mysql' | 'internal'

export interface DataSource {
  id: Uuid
  slug: string
  name: string
  description: string | null
  kind: SourceKind
  default_event_type: string | null
  mode: 'score' | 'load_only'
  connection: Record<string, unknown>
  inferred_schema: InferredSchema | null
  api_key_prefix: string | null
  is_active: boolean
  active_mapping_version?: number | null
  created_at: IsoDateTime
  api_key?: string // only returned once on create / rotate
}

export interface InferredField {
  path: string
  inferred_type: 'integer' | 'number' | 'string' | 'bool' | 'datetime' | 'array' | 'object'
  datetime_format?: string | null
  null_ratio: number
  distinct_ratio: number
  sample_values: unknown[]
  pii?: 'pan' | 'email' | 'phone' | 'account_number' | 'name' | null
}

export interface InferredSchema {
  fields: InferredField[]
  sampled_rows?: number
  inspected_at?: string
}

export interface InspectResult {
  upload_id?: string | null
  file_name?: string
  file_format?: string
  row_estimate?: number | null
  unmapped_fields?: string[]
  notes?: string[]
  llm_used?: boolean
  schema: InferredSchema
  suggested_mapping: MappingSpec
  confidence: Record<string, number>
  preview: Record<string, unknown>[]
}

export interface TransformStep {
  fn: string
  [param: string]: unknown
}

export interface FieldMapping {
  from?: string | string[]
  const?: unknown
  default?: unknown
  value_map?: Record<string, unknown>
  transform?: TransformStep[]
}

export interface MappingSpec {
  event_type?: FieldMapping
  event: Record<string, FieldMapping>
  customer?: Record<string, FieldMapping | Record<string, FieldMapping>>
  label?: { from: string, fraud_values: unknown[], fraud_type?: FieldMapping } | null
  drop_fields?: string[]
}

export interface MappingVersion {
  version: number
  mapping: MappingSpec
  status: 'draft' | 'active' | 'archived'
  created_at: IsoDateTime
  created_by?: Uuid | null
  activated_at: IsoDateTime | null
}

export interface MappingPreviewRow {
  ok: boolean
  event?: Record<string, unknown>
  customer?: Record<string, unknown>
  label?: Record<string, unknown> | null
  errors?: { field: string, message: string }[]
}

/** Several core-api endpoints return `{items}` without pagination. */
export interface Items<T> {
  items: T[]
  total?: number
}

export interface IngestJob {
  id: Uuid
  data_source_id: Uuid
  mode: 'score' | 'load_only'
  status: 'queued' | 'running' | 'done' | 'failed' | 'cancelled'
  total_rows: number | null
  processed_rows: number
  accepted_rows: number
  rejected_rows: number
  error: string | null
  created_at: IsoDateTime
  started_at: IsoDateTime | null
  finished_at: IsoDateTime | null
}

export interface IngestError {
  id: number
  job_id: Uuid | null
  record: Record<string, unknown>
  reason: string
  created_at: IsoDateTime
}

export interface FieldCatalogEntry {
  path: string
  entity: 'event' | 'source' | 'customer' | 'features' | 'ml' | 'graph'
  data_type: 'integer' | 'number' | 'string' | 'bool' | 'datetime' | 'array' | 'object' | 'category'
  description: string | null
  data_source_id: Uuid | null
  velocity_enabled: boolean
  /** column name to use in velocity group_by / aggregate (e.g. `device_id`), when velocity-enabled */
  velocity_column?: string | null
  pii: boolean
  builtin: boolean
}

export type { RuleDefinition, RuleEnvelope, RuleKind, RuleStatus }
