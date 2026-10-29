// Short haptic ticks for the on-screen buttons.
//
// Android and other Chromium browsers expose the Vibration API. iOS Safari (including installed
// PWAs) has no Vibration API, but since iOS 18 toggling a `<input type="checkbox" switch>` plays
// the system haptic, so a hidden one is clicked instead. Both only fire during a user gesture,
// so on iOS a tick plays when a finger lands, not when it slides onto another button.
//
// Chrome on Android passes the duration straight to the legacy one-shot vibrator at default
// amplitude. The LRA in phones like the Pixel 8 needs ~20 ms to spin up, so shorter pulses are
// dropped or imperceptible. Chrome also ignores vibrate() entirely while the ringer is on Silent,
// and Android scales it by Settings > Vibration & haptics > Touch feedback (off at zero).

let iosSwitch: HTMLLabelElement | null = null

function clickIosSwitch() {
  if (!iosSwitch) {
    iosSwitch = document.createElement('label')
    iosSwitch.setAttribute('aria-hidden', 'true')
    iosSwitch.style.display = 'none'
    const input = document.createElement('input')
    input.type = 'checkbox'
    input.tabIndex = -1
    input.setAttribute('switch', '')
    iosSwitch.append(input)
    document.head.append(iosSwitch)
  }
  iosSwitch.click()
}

/** Plays a short tick for a button press. */
export function hapticTick() {
  if (typeof navigator.vibrate === 'function') navigator.vibrate(30)
  else clickIosSwitch()
}
