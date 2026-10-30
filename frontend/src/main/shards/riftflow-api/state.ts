import type {
  RiftFlowAutoSelectGroupsConfig,
  RiftFlowContactChannels,
  RiftFlowFeatureGateSnapshot,
  RiftFlowLeagueServersConfig,
  RiftFlowNotice,
  RiftFlowRelease,
  RiftFlowSupportedQueuesConfig
} from '@shared/shards/riftflow-api'
import { makeAutoObservable, observable } from 'mobx'

import {
  BUILTIN_AUTO_SELECT_GROUPS,
  BUILTIN_SGP_LEAGUE_SERVERS_CONFIG,
  BUILTIN_SUPPORTED_QUEUES
} from './builtin'

export class RiftFlowApiState {
  featureGates: RiftFlowFeatureGateSnapshot | null = null
  leagueServers = BUILTIN_SGP_LEAGUE_SERVERS_CONFIG
  supportedQueues = BUILTIN_SUPPORTED_QUEUES
  autoSelectGroups = BUILTIN_AUTO_SELECT_GROUPS

  notice: RiftFlowNotice | null = null
  contactChannels: RiftFlowContactChannels | null = null
  latestRelease: RiftFlowRelease | null = null

  setLatestRelease(value: RiftFlowRelease | null) {
    this.latestRelease = value
  }

  isUpdatingNotice = false
  isUpdatingContactChannels = false
  isUpdatingLatestRelease = false
  isUpdatingFeatureGates = false
  isUpdatingLeagueServers = false
  isUpdatingSupportedQueues = false
  isUpdatingAutoSelectGroups = false

  setFeatureGates(value: RiftFlowFeatureGateSnapshot) {
    this.featureGates = value
  }

  setLeagueServers(value: RiftFlowLeagueServersConfig) {
    this.leagueServers = value
  }

  setSupportedQueues(value: RiftFlowSupportedQueuesConfig) {
    this.supportedQueues = value
  }

  setAutoSelectGroups(value: RiftFlowAutoSelectGroupsConfig) {
    this.autoSelectGroups = value
  }

  setNotice(value: RiftFlowNotice | null) {
    this.notice = value
  }

  setContactChannels(value: RiftFlowContactChannels | null) {
    this.contactChannels = value
  }

  setUpdatingNotice(value: boolean) {
    this.isUpdatingNotice = value
  }

  setUpdatingContactChannels(value: boolean) {
    this.isUpdatingContactChannels = value
  }

  setUpdatingLatestRelease(value: boolean) {
    this.isUpdatingLatestRelease = value
  }

  setUpdatingFeatureGates(value: boolean) {
    this.isUpdatingFeatureGates = value
  }

  setUpdatingLeagueServers(value: boolean) {
    this.isUpdatingLeagueServers = value
  }

  setUpdatingSupportedQueues(value: boolean) {
    this.isUpdatingSupportedQueues = value
  }

  setUpdatingAutoSelectGroups(value: boolean) {
    this.isUpdatingAutoSelectGroups = value
  }

  constructor() {
    makeAutoObservable(this, {
      featureGates: observable.ref,
      leagueServers: observable.ref,
      supportedQueues: observable.ref,
      autoSelectGroups: observable.ref,
      notice: observable.ref,
      contactChannels: observable.ref,
      latestRelease: observable.ref
    })
  }
}
