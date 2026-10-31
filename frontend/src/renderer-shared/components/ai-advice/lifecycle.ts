export class AdviceLifecycle {
  private readonly attempted = new Set<string>()
  private readonly dismissed = new Set<string>()
  canStart(key: string) {
    return !!key && !this.attempted.has(key) && !this.dismissed.has(key)
  }
  start(key: string) {
    if (!this.canStart(key)) return false
    this.attempted.add(key)
    return true
  }
  dismiss(key: string) {
    if (key) this.dismissed.add(key)
  }
  isDismissed(key: string) {
    return this.dismissed.has(key)
  }
}
