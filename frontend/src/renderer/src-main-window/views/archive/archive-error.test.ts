import { expect, it } from 'vitest'

import { archiveErrorMessage } from './archive-error'

it('shows actionable IPC messages instead of object coercion', () => {
  expect(archiveErrorMessage({ error: { message: '请先登录客户端' } }, '操作未完成')).toBe(
    '请先登录客户端'
  )
  expect(archiveErrorMessage(new Error('目录无法读取'), '操作未完成')).toBe('目录无法读取')
  expect(archiveErrorMessage({}, '操作未完成')).toBe('操作未完成')
  expect(archiveErrorMessage('[object Object]', '操作未完成')).toBe('操作未完成')
  const cyclic = { error: undefined as unknown }
  cyclic.error = cyclic
  expect(archiveErrorMessage(cyclic, '操作未完成')).toBe('操作未完成')
})
