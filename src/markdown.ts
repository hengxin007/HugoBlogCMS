export type Format = 'bold' | 'italic' | 'h1' | 'h2' | 'h3' | 'list' | 'ordered' | 'inline-code' | 'code' | 'quote' | 'link' | 'table'

type Edit = { from: number; to: number; insert: string; selectFrom: number; selectTo: number }

export function formatMarkdown(document: string, from: number, to: number, command: Format): Edit {
  const selected = document.slice(from, to)
  const inline = (before: string, value: string, after: string): Edit => ({
    from, to,
    insert: before + value + after,
    selectFrom: from + before.length,
    selectTo: from + before.length + value.length,
  })
  const block = (body: string, first: number, last: number): Edit => {
    const before = from > 0 && document[from - 1] !== '\n' ? '\n\n' : ''
    const after = to < document.length && document[to] !== '\n' ? '\n\n' : '\n'
    return { from, to, insert: before + body + after, selectFrom: from + before.length + first, selectTo: from + before.length + last }
  }
  if (command === 'bold') return inline('**', selected || '粗体文字', '**')
  if (command === 'italic') return inline('*', selected || '斜体文字', '*')
  if (command === 'link') {
    const label = selected.replace(/\n/g, ' ') || '链接名称'
    const url = 'https://example.com'
    const edit = inline('[', label, '](' + url + ')')
    if (selected) {
      edit.selectFrom = from + label.length + 3
      edit.selectTo = edit.selectFrom + url.length
    }
    return edit
  }
  let longest = 0
  for (const run of selected.matchAll(/\x60+/g)) longest = Math.max(longest, run[0].length)
  if (command === 'inline-code') {
    const ticks = '\x60'.repeat(longest + 1)
    const padding = selected.startsWith('\x60') || selected.endsWith('\x60') ? ' ' : ''
    return inline(ticks + padding, selected || '代码', padding + ticks)
  }
  if (command === 'code') {
    const fence = '\x60'.repeat(Math.max(longest + 1, 3))
    const value = selected || '在这里输入代码'
    return block(fence + '\n' + value + '\n' + fence, fence.length + 1, fence.length + 1 + value.length)
  }
  if (command === 'table') {
    const first = selected.trim().replace(/\s+/g, ' ').replace(/\|/g, '\\|') || '列名'
    return block('| ' + first + ' | 列名 |\n| --- | --- |\n| 内容 | 内容 |', 2, 2 + first.length)
  }
  const start = from === 0 ? 0 : document.lastIndexOf('\n', from - 1) + 1
  const lastSelected = to > from && document[to - 1] === '\n' ? to - 1 : to
  const next = document.indexOf('\n', lastSelected)
  const end = next === -1 ? document.length : next
  const original = document.slice(start, end)
  const prefix = (index: number) => {
    if (command === 'quote') return '> '
    if (command === 'list') return '- '
    if (command === 'ordered') return (index + 1) + '. '
    return '#'.repeat(Number(command[1])) + ' '
  }
  const lines = original.split('\n').map((line, index) => prefix(index) + (line || (original ? '' : '输入文字')))
  const insert = lines.join('\n')
  return { from: start, to: end, insert, selectFrom: start + prefix(0).length, selectTo: start + insert.length }
}
