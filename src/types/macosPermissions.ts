export interface MacosComputerPermissionsStatus {
  screenRecording: boolean
  screenRecordingPreflight: boolean
  accessibility: boolean
  appBundlePath: string
  executablePath: string
  bundleId: string
  runningFromAppBundle: boolean
}
