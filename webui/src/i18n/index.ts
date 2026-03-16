import { createI18n } from 'vue-i18n'
import { computed } from 'vue'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

type MessageSchema = typeof en

const savedLocale = localStorage.getItem('attaos-locale') || 'en'

const i18n = createI18n<[MessageSchema], 'en' | 'zh-CN'>({
  legacy: false,
  locale: savedLocale,
  fallbackLocale: 'en',
  messages: {
    en,
    'zh-CN': zhCN,
  },
})

// Set initial lang attribute on document
document.documentElement.setAttribute('lang', savedLocale)

/** Reactive ref to the current locale — safe to use in templates */
export const currentLocale = computed({
  get: () => (i18n.global.locale as unknown as { value: string }).value,
  set: (val: string) => {
    (i18n.global.locale as unknown as { value: string }).value = val
  },
})

export function setLocale(locale: string) {
  currentLocale.value = locale
  localStorage.setItem('attaos-locale', locale)
  document.documentElement.setAttribute('lang', locale)
}

export function getLocale(): string {
  return currentLocale.value
}

export default i18n
