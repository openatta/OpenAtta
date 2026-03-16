import { ofetch } from 'ofetch'
import { useNotificationStore } from '../stores/notification'

export function useApi() {
  const notificationStore = useNotificationStore()

  const client = ofetch.create({
    baseURL: '',
    headers: { 'Content-Type': 'application/json' },
    onResponseError({ response }) {
      const message =
        response._data?.message ||
        response._data?.error ||
        response.statusText ||
        `Request failed (${response.status})`
      notificationStore.add(String(message), 'error')
    },
  })

  async function get<T>(url: string): Promise<T> {
    return client<T>(url, { method: 'GET' })
  }

  async function post<T>(url: string, body: Record<string, any>): Promise<T> {
    return client<T>(url, { method: 'POST', body })
  }

  async function put<T>(url: string, body: Record<string, any>): Promise<T> {
    return client<T>(url, { method: 'PUT', body })
  }

  async function del(url: string): Promise<void> {
    return client(url, { method: 'DELETE' })
  }

  return { get, post, put, del }
}
