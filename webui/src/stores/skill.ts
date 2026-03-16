import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApi } from '../composables/useApi'
import type { SkillDef } from '../types/api'

export const useSkillStore = defineStore('skill', () => {
  const skills = ref<SkillDef[]>([])
  const loading = ref(false)
  const api = useApi()

  async function fetchSkills() {
    loading.value = true
    try {
      skills.value = await api.get<SkillDef[]>('/api/v1/skills')
    } finally {
      loading.value = false
    }
  }

  async function createSkill(skill: SkillDef) {
    return api.post<SkillDef>('/api/v1/skills', { skill })
  }

  async function updateSkill(id: string, skill: SkillDef) {
    return api.put<SkillDef>(`/api/v1/skills/${id}`, { skill })
  }

  async function deleteSkill(id: string) {
    return api.del(`/api/v1/skills/${id}`)
  }

  return { skills, loading, fetchSkills, createSkill, updateSkill, deleteSkill }
})
