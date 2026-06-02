/** User finished the wizard but macOS APIs may still report denied (TCC lag / wrong binary). */
const SESSION_ACK_KEY = 'pointer.macosComputerPerms.userAck'

export function setMacosComputerPermissionsUserAck(ack: boolean): void {
  try {
    if (ack) sessionStorage.setItem(SESSION_ACK_KEY, '1')
    else sessionStorage.removeItem(SESSION_ACK_KEY)
  } catch {
    /* private mode */
  }
}

export function hasMacosComputerPermissionsUserAck(): boolean {
  try {
    return sessionStorage.getItem(SESSION_ACK_KEY) === '1'
  } catch {
    return false
  }
}

export function clearMacosComputerPermissionsUserAck(): void {
  setMacosComputerPermissionsUserAck(false)
}
