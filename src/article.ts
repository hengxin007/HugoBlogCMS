import { isMap, isScalar, parseDocument, stringify } from 'yaml'

export type Article = { header: string; body: string; bom: string }

export function parseArticle(source: string): Article {
  const normalized = source.replace(/\r\n?/g, '\n')
  const match = /^(\uFEFF?)---[^\S\n]*\n([\s\S]*?)^---[^\S\n]*(?:\n|$)/m.exec(normalized)
  if (!match || match.index !== 0) throw new Error('文章需要以 YAML 元数据开头（---）。')
  return { header: match[2], body: normalized.slice(match[0].length), bom: match[1] }
}

export function serializeArticle(article: Article): string {
  return `---\n${article.header}---\n${article.body}`
}

export function readFields(header: string): Record<string, unknown> {
  const doc = parseDocument(header)
  if (doc.errors.length) throw new Error(doc.errors[0].message)
  const value = doc.toJS({ maxAliasCount: 30 })
  if (value !== null && (typeof value !== 'object' || Array.isArray(value))) throw new Error('YAML 元数据必须是键值表。')
  return value || {}
}

export function updateField(article: Article, field: string, value: unknown): Article {
  const doc = parseDocument(article.header)
  if (doc.errors.length) throw new Error(doc.errors[0].message)
  const formatted = stringify(value, { collectionStyle: 'flow', lineWidth: 0 }).trimEnd()
  const pair = isMap(doc.contents) ? doc.contents.items.find(item => isScalar(item.key) && item.key.value === field) : undefined
  if (pair && isScalar(pair.key) && pair.key.range) {
    const keyEnd = pair.key.range[1]
    const node = pair.value as { range?: [number, number, number] } | null
    const separator = article.header.indexOf(':', keyEnd) + 1
    const valueEnd = node?.range?.[1] ?? separator
    const previous = article.header.slice(separator, valueEnd)
    const after = article.header.slice(valueEnd)
    const newline = previous.endsWith('\n') && after && !after.startsWith('\n') ? '\n' : ''
    return { ...article, header: article.header.slice(0, separator) + ' ' + formatted + newline + after }
  }
  return { ...article, header: article.header + field + ': ' + formatted + '\n' }
}

export function copyArticle(source: string, slug: string, now = new Date()): string {
  let article = parseArticle(source)
  const header = parseDocument(article.header)
  if (header.errors.length) throw new Error(header.errors[0].message)
  // Drop routing and scheduling fields so the copy gets its own address and date.
  const obsolete = new Set(['url', 'aliases', 'publishDate', 'publishdate', 'expiryDate', 'expirydate', 'lastmod'])
  if (isMap(header.contents)) {
    const ranges = header.contents.items.flatMap(({ key, value }) => {
      if (!isScalar(key) || !key.range || !obsolete.has(String(key.value))) return []
      const end = (value as { range?: [number, number, number] } | null)?.range?.[2] ?? key.range[2]
      const after = end > 0 && article.header[end - 1] === '\n' ? end : article.header.indexOf('\n', end) + 1 || article.header.length
      return [{ from: article.header.lastIndexOf('\n', key.range[0] - 1) + 1, to: after }]
    })
    for (const { from, to } of ranges.reverse()) article.header = article.header.slice(0, from) + article.header.slice(to)
  }
  const title = readFields(article.header).title
  for (const [key, value] of Object.entries({ title: (title || '未命名文章') + '（副本）', slug, date: now.toISOString(), draft: true })) {
    article = updateField(article, key, value)
  }
  return serializeArticle(article)
}

export function releaseMarkdown(source: string): string {
  const article = parseArticle(source)
  const fields = readFields(article.header)
  if (!String(fields.title ?? '').trim()) throw new Error('请输入文章标题。')
  return fields.draft === true ? serializeArticle(updateField(article, 'draft', false)) : source
}

export function newArticle(slug: string, now = new Date()): string {
  return `---\ntitle: 未命名文章\nslug: ${slug}\ndate: ${now.toISOString()}\ndescription: ""\ndraft: true\ncategories: []\ntags: []\n---\n\n`
}

export function defaultSlug(now = new Date()): string {
  return `post-${now.toISOString().replace(/\D/g, '').slice(0, 17)}`
}

export function listItems(value: unknown): string[] {
  return Array.isArray(value) ? value.map(String) : typeof value === 'string' && value.trim() ? [value] : []
}
