export interface ChordMapping {
  keys: string[]
  output: string
  kind: 'syllable' | 'fragment'
}

export interface ChordProfile {
  version: number
  name: string
  left_keys: string[]
  right_keys: string[]
  interval_ms?: number
  mappings: ChordMapping[]
}

export interface ChordStroke {
  key: string
  down: boolean
  atMs: number
}

export interface ChordOutcome {
  keys: string[]
  output: string | null
  kind: ChordMapping['kind'] | null
  atMs: number
  idle: boolean
}

export type ChordError =
  | 'profile_limit'
  | 'invalid_profile'
  | 'empty_profile'
  | 'unsupported_key'
  | 'duplicate_key'
  | 'trace_limit'
  | 'invalid_trace'

export type RehearsalResponse =
  | { error: ChordError; key?: string }
  | { physical: Record<string, string>; intervalMs: number; maxStrokes: number; outcomes: ChordOutcome[] }

export type Scope = 'all' | 'single' | 'both'

export interface Drill {
  targets: ChordMapping[]
  index: number
  attempts: number
  firstHits: number
  solved: number
  tried: boolean
}

const ROUND_SIZE = 8

// KeyboardEvent.code follows the physical key position. Convert that position to
// the US-style identifier sent by the core, independent of modifiers and IME text.
const symbols: Readonly<Record<string, string>> = {
  Backquote: '`',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
}

export function physicalKey(code: string): string | undefined {
  return /^Key[A-Z]$/.test(code) ? code[3]?.toLowerCase() : symbols[code]
}

export function shortKey(key: string, physical: Record<string, string>): string {
  const position = Object.keys(physical).find((code) => physical[code] === key)
  return position?.toUpperCase() ?? key
}

export function drillPool(profile: ChordProfile, scope: Scope): ChordMapping[] {
  const left = new Set(profile.left_keys)
  const right = new Set(profile.right_keys)
  return profile.mappings.filter((row) => {
    if (scope === 'all') return true
    const both = row.keys.some((key) => left.has(key)) && row.keys.some((key) => right.has(key))
    return scope === 'both' ? both : !both
  })
}

export function startDrill(pool: ChordMapping[], round: number): Drill {
  const start = pool.length ? (ROUND_SIZE * round) % pool.length : 0
  return {
    targets: [...pool.slice(start), ...pool.slice(0, start)].slice(0, ROUND_SIZE),
    index: 0,
    attempts: 0,
    firstHits: 0,
    solved: 0,
    tried: false,
  }
}

export function matches(target: ChordMapping, outcomes: ChordOutcome[]): boolean {
  const result = outcomes[0]
  if (outcomes.length !== 1 || !result) return false
  const expected = new Set(target.keys)
  return (
    result.output === target.output &&
    result.kind === target.kind &&
    result.keys.length === expected.size &&
    result.keys.every((key) => expected.has(key))
  )
}

export function scoreAttempt(drill: Drill, outcomes: ChordOutcome[]): boolean {
  const target = drill.targets[drill.index]
  if (!target) return false
  drill.attempts++
  if (!matches(target, outcomes)) {
    drill.tried = true
    return false
  }
  if (!drill.tried) drill.firstHits++
  drill.solved++
  advance(drill)
  return true
}

export function advance(drill: Drill): void {
  if (drill.index < drill.targets.length) drill.index++
  drill.tried = false
}
