import { describe, expect, test } from 'vitest'

import { createApplyUpdaterArguments, createUninstallUpdaterArguments } from './updater-command'

describe('updater command arguments', () => {
  test('keeps a volume root target in its own argv entry', () => {
    expect(
      createApplyUpdaterArguments({
        locale: 'zh-CN',
        archivePath: String.raw`C:\Users\Administrator\AppData\Roaming\riftflow\NewUpdates\riftflow-win-x64.7z`,
        targetPath: 'F:\\'
      })
    ).toEqual([
      '--lang',
      'zh-CN',
      '--executable',
      'RiftFlow.exe',
      'apply',
      '--archive',
      String.raw`C:\Users\Administrator\AppData\Roaming\riftflow\NewUpdates\riftflow-win-x64.7z`,
      '--target',
      'F:\\',
      '--delete-archive',
      '--launch'
    ])
  })

  test('passes uninstall paths containing spaces without command-shell quotes', () => {
    const args = createUninstallUpdaterArguments({
      locale: 'en',
      appIds: ['riftflow', 'riftflow-dev'],
      appPath: String.raw`C:\RiftFlow`,
      dataPath: String.raw`C:\Users\RiftFlow User\AppData\Roaming\riftflow`
    })

    expect(args).toContain(String.raw`C:\RiftFlow`)
    expect(args).toContain(String.raw`C:\Users\RiftFlow User\AppData\Roaming\riftflow`)
    expect(args.every((arg) => !arg.includes('"'))).toBe(true)
  })
})
