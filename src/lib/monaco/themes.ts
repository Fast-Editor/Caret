import type * as Monaco from 'monaco-editor'

type Rules = Monaco.editor.ITokenThemeRule[]

const darkRules = (p: { comment: string; keyword: string; string: string; number: string; type: string; fn: string; variable: string; tag: string; attr: string; regexp: string; operator: string; constant: string; punctuation: string }): Rules => [
  { token: '', foreground: p.variable },
  { token: 'comment', foreground: p.comment, fontStyle: 'italic' },
  { token: 'comment.doc', foreground: p.comment, fontStyle: 'italic' },
  { token: 'keyword', foreground: p.keyword },
  { token: 'keyword.control', foreground: p.keyword },
  { token: 'keyword.operator', foreground: p.operator },
  { token: 'storage', foreground: p.keyword },
  { token: 'storage.type', foreground: p.keyword },
  { token: 'string', foreground: p.string },
  { token: 'string.escape', foreground: p.constant },
  { token: 'string.key.json', foreground: p.attr },
  { token: 'string.value.json', foreground: p.string },
  { token: 'number', foreground: p.number },
  { token: 'number.hex', foreground: p.number },
  { token: 'constant', foreground: p.constant },
  { token: 'constant.language', foreground: p.constant },
  { token: 'regexp', foreground: p.regexp },
  { token: 'type', foreground: p.type },
  { token: 'type.identifier', foreground: p.type },
  { token: 'entity.name.type', foreground: p.type },
  { token: 'entity.name.class', foreground: p.type },
  { token: 'entity.name.function', foreground: p.fn },
  { token: 'support.function', foreground: p.fn },
  { token: 'identifier', foreground: p.variable },
  { token: 'variable', foreground: p.variable },
  { token: 'variable.predefined', foreground: p.constant },
  { token: 'variable.parameter', foreground: p.attr },
  { token: 'tag', foreground: p.tag },
  { token: 'tag.id.pug', foreground: p.tag },
  { token: 'metatag', foreground: p.tag },
  { token: 'metatag.content.html', foreground: p.string },
  { token: 'attribute.name', foreground: p.attr },
  { token: 'attribute.value', foreground: p.string },
  { token: 'attribute.value.number.css', foreground: p.number },
  { token: 'attribute.value.unit.css', foreground: p.number },
  { token: 'delimiter', foreground: p.punctuation },
  { token: 'delimiter.html', foreground: p.punctuation },
  { token: 'delimiter.bracket', foreground: p.punctuation },
  { token: 'operator', foreground: p.operator },
  { token: 'operators', foreground: p.operator },
  { token: 'annotation', foreground: p.constant },
  { token: 'namespace', foreground: p.type },
  { token: 'predefined', foreground: p.constant },
  { token: 'key', foreground: p.attr },
  { token: 'emphasis', fontStyle: 'italic' },
  { token: 'strong', fontStyle: 'bold' },
  { token: 'keyword.md', foreground: p.keyword, fontStyle: 'bold' },
  { token: 'string.link.md', foreground: p.fn },
  { token: 'variable.md', foreground: p.string },
  { token: 'meta.diff.header', foreground: p.comment },
  { token: 'meta.diff.range', foreground: p.keyword },
  { token: 'meta.diff.add', foreground: '9fe6bd' },
  { token: 'meta.diff.delete', foreground: 'ff9a95' },
  { token: 'invalid', foreground: 'f2655f' },
]

export const caretDark: Monaco.editor.IStandaloneThemeData = {
  base: 'vs-dark', inherit: true,
  rules: darkRules({ comment: '6a737d', keyword: 'c586c0', string: 'ce9178', number: 'b5cea8', type: '4ec9b0', fn: 'dcdcaa', variable: 'd4d4d4', tag: '569cd6', attr: '9cdcfe', regexp: 'd16969', operator: 'd4d4d4', constant: '569cd6', punctuation: 'c8c8c8' }),
  colors: {
    'editor.background': '#1a1a1a', 'editor.foreground': '#cccccc', 'editorLineNumber.foreground': '#5a5a5a', 'editorLineNumber.activeForeground': '#c6c6c6',
    'editorCursor.foreground': '#aeafad', 'editor.selectionBackground': '#264f78aa', 'editor.inactiveSelectionBackground': '#3a3d4160', 'editor.lineHighlightBackground': '#ffffff07', 'editor.lineHighlightBorder': '#00000000',
    'editorIndentGuide.background1': '#2a2a2a', 'editorIndentGuide.activeBackground1': '#444444', 'editorWhitespace.foreground': '#3a3a3a', 'editorBracketMatch.background': '#3a3a3a80', 'editorBracketMatch.border': '#4daafc66',
    'editorWidget.background': '#1f1f1f', 'editorWidget.border': '#2a2a2a', 'editorSuggestWidget.background': '#1f1f1f', 'editorSuggestWidget.border': '#2a2a2a', 'editorSuggestWidget.selectedBackground': '#ffffff1a', 'editorHoverWidget.background': '#1f1f1f', 'editorHoverWidget.border': '#2a2a2a',
    'editorGutter.background': '#1a1a1a', 'editorGutter.addedBackground': '#6fcf97', 'editorGutter.modifiedBackground': '#4daafc', 'editorGutter.deletedBackground': '#f14c4c',
    'diffEditor.insertedTextBackground': '#6fcf9726', 'diffEditor.removedTextBackground': '#f14c4c26', 'diffEditor.insertedLineBackground': '#6fcf9714', 'diffEditor.removedLineBackground': '#f14c4c14',
    'editorOverviewRuler.border': '#00000000', 'scrollbarSlider.background': '#ffffff14', 'scrollbarSlider.hoverBackground': '#ffffff24', 'scrollbarSlider.activeBackground': '#ffffff30', 'scrollbar.shadow': '#00000000',
    'minimap.background': '#1a1a1a', 'editorError.foreground': '#f14c4c', 'editorWarning.foreground': '#e2b35b', 'editorInfo.foreground': '#4daafc', 'editorGhostText.foreground': '#9a9a9a', 'editorGhostText.border': '#00000000',
    'editorInlayHint.background': '#2a2a2a80', 'editorInlayHint.foreground': '#9a9a9a', 'editorStickyScroll.background': '#1a1a1a', 'editorStickyScroll.shadow': '#00000040', 'editorCodeLens.foreground': '#7a7a7a',
    'editorLink.activeForeground': '#4daafc', 'input.background': '#1f1f1f', 'input.border': '#2a2a2a', 'focusBorder': '#6a6a6a', 'list.hoverBackground': '#ffffff0f', 'list.activeSelectionBackground': '#ffffff1a', 'quickInput.background': '#1f1f23', 'menu.background': '#1f1f1f', 'menu.selectionBackground': '#ffffff1a',
    'editorMarkerNavigation.background': '#1f1f1f', 'peekView.border': '#4daafc', 'peekViewEditor.background': '#181818', 'peekViewResult.background': '#181818', 'peekViewTitle.background': '#181818',
  },
}

export const caretMidnight: Monaco.editor.IStandaloneThemeData = {
  base: 'vs-dark', inherit: true,
  rules: darkRules({ comment: '5c6773', keyword: 'ff7b72', string: 'a5d6ff', number: '79c0ff', type: 'ffa657', fn: 'd2a8ff', variable: 'd7dee8', tag: '7ee787', attr: '79c0ff', regexp: 'a5d6ff', operator: 'ff7b72', constant: '79c0ff', punctuation: 'c9d1d9' }),
  colors: { ...caretDark.colors, 'editor.background': '#0d1117', 'editorGutter.background': '#0d1117', 'minimap.background': '#0d1117', 'editorStickyScroll.background': '#0d1117', 'editor.foreground': '#d7dee8', 'editorWidget.background': '#161c25', 'editorSuggestWidget.background': '#161c25', 'editorHoverWidget.background': '#161c25', 'editorLineNumber.foreground': '#3b4552', 'editor.selectionBackground': '#264f7888', 'editorIndentGuide.background1': '#1f2733', 'quickInput.background': '#0f141b', 'menu.background': '#161c25', 'peekViewEditor.background': '#0f141b', 'input.background': '#161c25' },
}

export const caretLight: Monaco.editor.IStandaloneThemeData = {
  base: 'vs', inherit: true,
  rules: darkRules({ comment: '6a737d', keyword: 'd73a49', string: '032f62', number: '005cc5', type: '6f42c1', fn: '6f42c1', variable: '24292e', tag: '22863a', attr: '005cc5', regexp: '032f62', operator: 'd73a49', constant: '005cc5', punctuation: '24292e' }),
  colors: {
    'editor.background': '#ffffff', 'editor.foreground': '#1f2328', 'editorLineNumber.foreground': '#b0b4ba', 'editorLineNumber.activeForeground': '#57606a', 'editorCursor.foreground': '#1f2328',
    'editor.selectionBackground': '#add6ff', 'editor.inactiveSelectionBackground': '#e5ebf1', 'editor.lineHighlightBackground': '#f6f8fa', 'editorIndentGuide.background1': '#eaeef2', 'editorIndentGuide.activeBackground1': '#d0d7de',
    'editorWidget.background': '#ffffff', 'editorWidget.border': '#dcdce2', 'editorSuggestWidget.selectedBackground': '#ddf4ff', 'editorGutter.addedBackground': '#1a7f37', 'editorGutter.modifiedBackground': '#2f6fed', 'editorGutter.deletedBackground': '#d1242f',
    'diffEditor.insertedTextBackground': '#1a7f3722', 'diffEditor.removedTextBackground': '#d1242f22', 'editorGhostText.foreground': '#8c959f', 'editorError.foreground': '#d1242f', 'editorWarning.foreground': '#9a6700', 'editorInfo.foreground': '#0969da', 'focusBorder': '#2f6fed', 'scrollbarSlider.background': '#00000018', 'scrollbarSlider.hoverBackground': '#00000028', 'editorStickyScroll.background': '#ffffff', 'minimap.background': '#ffffff',
  },
}

export const caretHighContrast: Monaco.editor.IStandaloneThemeData = {
  base: 'hc-black', inherit: true,
  rules: darkRules({ comment: 'a0a0a0', keyword: 'ff9cf0', string: 'ffd68a', number: 'b5f5a8', type: '7fe9ff', fn: 'ffff80', variable: 'ffffff', tag: '8fd3ff', attr: 'bfe9ff', regexp: 'ffb3b3', operator: 'ffffff', constant: 'bfe9ff', punctuation: 'ffffff' }),
  colors: { 'editor.background': '#000000', 'editor.foreground': '#ffffff', 'editorCursor.foreground': '#ffffff', 'editorLineNumber.foreground': '#a0a0a0', 'focusBorder': '#4cc2ff', 'editor.selectionBackground': '#4cc2ff55', 'editorGhostText.foreground': '#b0b0b0' },
}

export const THEMES: Record<string, Monaco.editor.IStandaloneThemeData> = {
  'caret-dark': caretDark, 'caret-midnight': caretMidnight, 'caret-light': caretLight, 'caret-high-contrast': caretHighContrast,
}
