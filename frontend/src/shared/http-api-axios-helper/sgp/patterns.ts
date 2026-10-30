export const URL_PLACEHOLDER_SUB_ID = '@riftflow:sgpServerSubId@'
export const RIFTFLOW_HEADER_SGP_SERVER_ID = 'x-riftflow-sgp-server-id'
export const RIFTFLOW_HEADER_TOKEN_TYPE = 'x-riftflow-token-type'

/**
 * 部分接口用  Transfer-Encoding: chunked 会出错
 *
 * 故用此 Header 来标记应该强制收集流并发送
 */
export const RIFTFLOW_HEADER_FORCE_STREAM_COLLECT = 'x-riftflow-force-stream-collect'
