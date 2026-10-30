import { is } from '@electron-toolkit/utils'

export const DEEP_LINK_PROTOCOL = is.dev ? 'riftflow-dev' : 'riftflow'

export const DEEP_LINK_PROTOCOL_PROD = 'riftflow'

export const DEEP_LINK_PROTOCOL_DEV = 'riftflow-dev'
