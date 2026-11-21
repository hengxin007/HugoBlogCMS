export type ThemeMode = 'system' | 'light' | 'dark' | 'custom'

export type Preferences = {
  disableGpu: boolean
  disableAnimations: boolean
  proxy: string
  hugoPath: string
  previewPort: number | null
  hostedImages: boolean | null
  editorFontSize: number
  interfaceFontSize: number
  theme: ThemeMode
  accentColor: string
  customBase: Exclude<ThemeMode, 'custom'>
}

export const defaultAccent = '#a5235e'

export type HugoInfo = { path: string; version: string; extended: boolean }

export const defaults: Preferences = {
  disableGpu: false, disableAnimations: false,
  proxy: '', hugoPath: '', previewPort: null, hostedImages: null,
  editorFontSize: 13, interfaceFontSize: 14, theme: 'system',
  accentColor: defaultAccent, customBase: 'system',
}

export function accentText(hex: string) {
  const linear = (offset: number) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  }
  const luminance = 0.2126 * linear(1) + 0.7152 * linear(3) + 0.0722 * linear(5)
  return luminance > 0.179 ? '#000' : '#fff'
}
