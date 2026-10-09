import type { RuleEnvelope } from '#shared/rules/dsl'

/** Methods exposed by <RuleEditor> via template ref. */
export interface RuleEditorExpose {
  validate: () => Promise<boolean>
  getEnvelope: () => RuleEnvelope
}
