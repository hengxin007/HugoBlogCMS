import assert from 'node:assert/strict'
import { test } from 'node:test'
import { copyArticle, defaultSlug, newArticle, parseArticle, readFields, releaseMarkdown, serializeArticle, updateField } from './article.ts'
import { formatMarkdown } from './markdown.ts'

test('preserves existing shortcode, custom metadata and slug when editing a title', () => {
  const raw = '\uFEFF---\r\n# 注释\r\ntitle: "原始标题"\r\nslug: 002\r\nai: false\r\n---\r\n{{< notice >}}\r\n![旧图](https://example.com/old.png)\r\n'
  const article = updateField(parseArticle(raw), 'title', '中文标题')
  const serialized = serializeArticle(article)
  assert.match(serialized, /slug: 002/)
  assert.match(serialized, /# 注释/)
  assert.match(serialized, /{{< notice >}}/)
  assert.doesNotMatch(serialized, /^\uFEFF/)
  assert.equal(readFields(article.header).ai, false)
})

test('rejects missing or invalid front matter', () => {
  assert.throws(() => parseArticle('正文\n---\ntitle: bad\n---\n'))
  assert.throws(() => readFields('title: ['))
})

test('creates valid Hugo drafts', () => {
  const now = new Date('2026-11-21T01:00:00.123Z')
  const fields = readFields(parseArticle(newArticle(defaultSlug(now), now)).header)
  assert.equal(fields.slug, 'post-20261121010000123')
  assert.equal(fields.draft, true)
})

test('changes a block list and leaves neighboring YAML values intact', () => {
  const source = '---\ntags:\n  - old\n  - next\nslug: 003 # existing\ndraft: false\n---\n正文'
  const changed = updateField(parseArticle(source), 'tags', ['新标签', 'Vue'])
  assert.deepEqual(readFields(changed.header).tags, ['新标签', 'Vue'])
  assert.match(changed.header, /slug: 003 # existing/)
})

test('duplication resets routing and scheduled dates while preserving custom metadata and body', () => {
  const raw = '---\nurl: /p/old/\naliases:\n  - /legacy/\nslug: 002\ntitle: 旧文\npublishDate: 2099-01-01\ncustom: true\n---\n![图片](image.png)\n'
  const copy = parseArticle(copyArticle(raw, 'fresh', new Date('2026-11-21T00:00:00Z')))
  const fields = readFields(copy.header)
  assert.equal(fields.url, undefined)
  assert.equal(fields.aliases, undefined)
  assert.equal(fields.publishDate, undefined)
  assert.equal(fields.slug, 'fresh')
  assert.equal(fields.custom, true)
  assert.equal(fields.title, '旧文（副本）')
  assert.equal(fields.draft, true)
  assert.equal(copy.body, '![图片](image.png)\n')
})

test('release changes only the Hugo draft flag in the temporary Markdown', () => {
  const local = '---\ntitle: 测试\ndraft: true # local\nslug: 002\n---\n正文\n'
  assert.equal(releaseMarkdown(local), local.replace('true', 'false'))
  assert.equal(readFields(parseArticle(local).header).draft, true)
})

test('formats selected content and leaves the next unselected line alone', () => {
  assert.deepEqual(formatMarkdown('一二三', 1, 2, 'bold'), { from: 1, to: 2, insert: '**二**', selectFrom: 3, selectTo: 4 })
  assert.equal(formatMarkdown('第一项\n第二项\n正文', 0, 8, 'ordered').insert, '1. 第一项\n2. 第二项')
  assert.equal(formatMarkdown('', 0, 0, 'table').insert, '| 列名 | 列名 |\n| --- | --- |\n| 内容 | 内容 |\n')
})
