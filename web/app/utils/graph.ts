/**
 * Graph colours shared by the Cytoscape canvas and its legend, so the legend always matches what is drawn.
 * Each entity kind has a distinct hue; customer colours encode the risk label.
 */
export const GRAPH_ENTITY_COLORS: Record<string, string> = {
  email: '#0ea5e9', phone: '#8b5cf6', device: '#f59e0b', ip: '#64748b', card: '#ec4899',
  bank_account: '#14b8a6', address: '#84cc16', ref_transaction: '#f97316', api_client: '#a16207',
}

export const GRAPH_CUSTOMER_COLORS = {
  unknown: '#6366f1',
  fraud: '#e11d48',
  legit: '#10b981',
  center: '#facc15',
  similar: '#f59e0b',
  path: '#e11d48',
} as const

export const graphEntityColor = (kind: string | null | undefined) => GRAPH_ENTITY_COLORS[kind ?? ''] ?? '#94a3b8'
