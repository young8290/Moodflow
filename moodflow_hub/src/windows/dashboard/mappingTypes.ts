export interface MappingRow {
  word: string
  added: string[]
  hidden: string[]
}

export interface MappingChange {
  before: MappingRow
  after: MappingRow
}

export interface MappingPreview {
  builtin: string[]
  effective: string[]
  enabled: boolean
  scope: string
  show_as: string
}

export interface MappingPage {
  rows: MappingRow[]
  total: number
  offset: number
  limit: number
}

export interface ImportPlan {
  edits: MappingChange[]
  new_count: number
  conflict_count: number
  skipped_count: number
  unchanged_count: number
}

export type ImportPolicy = 'skip' | 'merge' | 'replace'

export const mappingKey = (symbol: string) => symbol.replaceAll('\uFE0F', '')

export function mappingRow(row: Partial<MappingRow> & { word: string }): MappingRow {
  return { word: row.word, added: [...(row.added ?? [])], hidden: [...(row.hidden ?? [])] }
}
