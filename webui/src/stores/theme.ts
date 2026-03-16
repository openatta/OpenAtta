import { defineStore } from 'pinia'

export const useThemeStore = defineStore('theme', {
  state: () => ({
    mode: (localStorage.getItem('atta-theme') || 'light') as 'light' | 'dark'
  }),
  actions: {
    toggle() {
      this.mode = this.mode === 'light' ? 'dark' : 'light'
      localStorage.setItem('atta-theme', this.mode)
      this.apply()
    },
    apply() {
      document.documentElement.classList.toggle('dark', this.mode === 'dark')
    }
  }
})
