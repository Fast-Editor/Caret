import { describe, it, expect } from 'vitest'
import { isDialogUnavailable, DIALOG_UNAVAILABLE } from './dialogs'
import { CaretApiError } from './tauri'

describe('isDialogUnavailable', () => {
  it('matches only the dialog_unavailable Rust error', () => {
    expect(isDialogUnavailable(new CaretApiError({ code: DIALOG_UNAVAILABLE, message: 'no panel' }))).toBe(true)
    expect(isDialogUnavailable(new CaretApiError({ code: 'not_found', message: 'x' }))).toBe(false)
    expect(isDialogUnavailable(new Error('dialog_unavailable'))).toBe(false)
    expect(isDialogUnavailable(null)).toBe(false)
  })
})
