import { Box, Braces, Code2, FunctionSquare, Hash, Key, LayoutList, Package, Parentheses, Type, Variable, Workflow, Component, Zap, Boxes, Layers } from 'lucide-react'
import { monaco } from '@/lib/monaco/setup'
const K = monaco.languages.SymbolKind
export function SymbolIcon({ kind, size = 12 }: { kind: monaco.languages.SymbolKind; size?: number }) {
  const cls = 'shrink-0'
  switch (kind) {
    case K.File: return <Code2 size={size} className={cls} />
    case K.Module: case K.Namespace: case K.Package: return <Package size={size} className={`${cls} text-warning`} />
    case K.Class: case K.Struct: return <Box size={size} className={`${cls} text-warning`} />
    case K.Method: case K.Function: case K.Constructor: return <FunctionSquare size={size} className={`${cls} text-accent`} />
    case K.Property: case K.Field: return <Key size={size} className={`${cls} text-info`} />
    case K.Interface: return <Component size={size} className={`${cls} text-info`} />
    case K.Enum: case K.EnumMember: return <LayoutList size={size} className={`${cls} text-warning`} />
    case K.Variable: case K.Constant: return <Variable size={size} className={`${cls} text-info`} />
    case K.String: return <Hash size={size} className={cls} />
    case K.Number: case K.Boolean: return <Zap size={size} className={cls} />
    case K.Array: case K.Object: return <Braces size={size} className={cls} />
    case K.Event: return <Workflow size={size} className={cls} />
    case K.Operator: return <Parentheses size={size} className={cls} />
    case K.TypeParameter: return <Type size={size} className={cls} />
    case K.Key: return <Key size={size} className={cls} />
    case K.Null: return <Boxes size={size} className={cls} />
    default: return <Layers size={size} className={cls} />
  }
}
