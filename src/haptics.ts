// Short haptic ticks for the on-screen buttons.
//
// Android and other Chromium browsers expose the Vibration API. iOS Safari (including installed
// PWAs) has no Vibration API, but since iOS 18 toggling a `<input type="checkbox" switch>` plays
// the system haptic, so a hidden one is clicked instead. Both only fire during a user gesture,
// so on iOS a tick plays when a finger lands, not when it slides onto another button.

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

/** Plays a short tick; `strong` for a new press, light for a D-pad direction change. */
export function hapticTick(strong = true) {
  if (typeof navigator.vibrate === 'function') navigator.vibrate(strong ? 14 : 8)
  else clickIosSwitch()
}
