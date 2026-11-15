// The native scorer uses Levenshtein distance on Unicode scalar values. Use the
// same edit costs here to show where the final text diverged from the scored span.
export type ChangeKind = 'match' | 'replace' | 'missing' | 'extra'

export interface ReviewPart {
  kind: ChangeKind
  expected: string
  actual: string
}

export function reviewText(target: string, typed: string): ReviewPart[] {
  const expected = [...target]
  const actual = [...typed]
  const width = actual.length + 1
  const at = (i: number, j: number) => i * width + j
  const distance = new Uint16Array((expected.length + 1) * width)
  for (let i = 0; i <= expected.length; i++) distance[at(i, 0)] = i
  for (let j = 0; j <= actual.length; j++) distance[at(0, j)] = j
  for (let i = 1; i <= expected.length; i++) {
    for (let j = 1; j <= actual.length; j++) {
      distance[at(i, j)] = Math.min(
        distance[at(i - 1, j - 1)]! + Number(expected[i - 1] !== actual[j - 1]),
        distance[at(i - 1, j)]! + 1,
        distance[at(i, j - 1)]! + 1,
      )
    }
  }

  const parts: ReviewPart[] = []
  let i = expected.length
  let j = actual.length
  while (i > 0 || j > 0) {
    const left = expected[i - 1]
    const right = actual[j - 1]
    const value = distance[at(i, j)]
    if (i > 0 && j > 0 && value === distance[at(i - 1, j - 1)]! + Number(left !== right)) {
      parts.push({ kind: left === right ? 'match' : 'replace', expected: left!, actual: right! })
      i--
      j--
    } else if (i > 0 && value === distance[at(i - 1, j)]! + 1) {
      parts.push({ kind: 'missing', expected: left!, actual: '' })
      i--
    } else {
      parts.push({ kind: 'extra', expected: '', actual: right! })
      j--
    }
  }
  return parts.reverse().reduce<ReviewPart[]>((groups, part) => {
    const last = groups.at(-1)
    if (last?.kind === part.kind) {
      last.expected += part.expected
      last.actual += part.actual
    } else groups.push(part)
    return groups
  }, [])
}
