# ClaudeButton

Repurpose weird HID devices into Claude control. Grab a cheap BLE remote / ring /
dial, translate its events into keystrokes, and inject them into whatever window
is focused — so you can drive a Claude Code session from a physical button.

<p align="center">
  <img src="assets/goodring.png" alt="The ClaudeButton ring — a 5-button D-pad ring with a touch strip" width="480">
</p>

## How it works

A single root daemon:
1. **Discovers & grabs** the input nodes of any device a profile claims (matched
   by evdev name). Grabbing means the device's native events (volume, cursor…)
   never reach the desktop — no side effects.
2. **Translates** each device's events with a built-in *translator* (see below).
3. **Injects** the resulting keystrokes through one shared uinput virtual
   keyboard (declared with a full key range so libinput routes it to the focused
   window).
4. **Survives** the cheap gadgets: one reader thread per device, re-grab on
   re-enumeration, and an optional BLE reconnect nudge when a device sleeps.

## Profiles are data, not code

Every device is a file in `profiles/` (`*.yaml`, `*.yml`, or `*.json`). It names
one of the built-in **translators** and supplies its parameters + key map. A new
device that fits an existing translator needs **no recompile** — just a new file.

Translators:
- **`gesture`** — device fakes a touchpad (canned swipe strokes). Classifies each
  stroke into `up/down/left/right/tap` and maps those to keys. Params: `tap_dist`,
  `map`.
- **`keymap`** — device sends real key codes; remap input key → output key,
  fired on press (value 1). Params: `map` (e.g. `KEY_VOLUMEUP: KEY_UP`, or a
  gamepad `BTN_EAST: KEY_ENTER`).
- **`click`** — like `keymap`, but fires only on a *clean tap*: a key's press
  (value 1) immediately followed by its release (value 0). Holding the button
  autorepeats (value 2) between the press and release, so a **hold never fires** —
  this is how a quick tap is told apart from a long-press on the same key. Params:
  `map`. (This is what turns the ring's long-press into the `continue` macro
  below — the right button emits a `KEY_POWER` pulse that a clean tap catches.)
- **`axis`** — device has absolute axes (joystick stick / hat / dpad). Map an
  axis + direction (`ABS_HAT0Y-`) to a key, fired once when the axis enters that
  zone. Params: `center`, `threshold` (dead zone; hat uses `0`/`1`, an analog
  stick `0..255` uses `center: 127, threshold: 64`), `map`.

**Map values** are output key names (`KEY_ENTER`) — or, with a `type:` prefix,
**literal text to type**. A `type:` value taps each character in turn (Shift held
for uppercase) and treats `\n` as Enter, so `KEY_POWER: "type:continue\n"` types
`continue` and presses Enter. This works in any translator's `map`.

A profile may use a single `translator:` or a list of `translators:` (evaluated
in order, first match wins) — so one device can mix algorithms, e.g. a gamepad's
stick via `axis` and its buttons via `keymap`, or the ring's swipes via `gesture`
plus a long-press macro via `click`.

Add a new translator kind only for a genuinely novel device; it's a small enum
arm in `src/profile.rs`.

### Example: `profiles/jx05.yaml`

```yaml
name: jx05
match:
  name_prefix: "JX-05"
reconnect:
  mac: "A7:B6:5E:94:60:D5"
  adapter: "00:1A:7D:DA:71:13"   # CSR dongle (antenna) — NOT the antenna-less Intel

# A clean tap of the right button's KEY_POWER pulse (a long-press, distinct from
# the swipe) types "continue" + Enter. A hold autorepeats and won't fire.
translators:
  - kind: click
    map:
      KEY_POWER: "type:continue\n"

translator:
  kind: gesture
  tap_dist: 400
  map:
    up: KEY_UP
    down: KEY_DOWN
    tap: KEY_ENTER      # center button
    left: KEY_ESC
    right: KEY_RIGHT    # accept autocomplete
```

## Onboard a new device

1. Pair it (BLE) or plug it in.
2. Scope what it emits with the built-in capture mode (grabs every node whose
   name contains the substring, prints raw events + gesture classification):

   ```
   sudo ./target/release/claudebutton capture JX-05
   ```

   Operate the device one action at a time and note whether it sends discrete
   `KEY`s (→ `keymap`, or `click` if you want tap-only and not hold) or
   `ABS`/`GESTURE` swipes (→ `gesture`).
3. Write `profiles/<name>.yaml` (or `.json`) with a `match` and the right
   translator + key map. Restart the service. Done — no recompile.

## Run

```
cargo build --release
sudo ./target/release/claudebutton         # foreground, from the project dir
# or install as a service (supersedes the old python jx05-remote):
./service/install.sh
journalctl -u claudebutton -f              # watch presses live
```

Runs as root: reads `/dev/input` and writes `/dev/uinput` with no group/ACL/udev
setup and no relogin; a root-created uinput keyboard reaches the active X session.

## Devices

| Profile | Device | Notes |
|---|---|---|
| jx05 | JX-05 BLE ring | 5-button D-pad; Jieli firmware fakes a touchpad (`gesture`). Right-button long-press → `continue` macro via `click` on its `KEY_POWER` pulse. |
| mocute | MOCUTE-032 BLE gamepad | Game mode: thumbstick reports as a hat (`axis`) + face buttons (`keymap`). |

## Roadmap

- Swap the `bluetoothctl` reconnect shell-out for the **`bluer`** crate — native
  connect + disconnect **signals** (drop the /proc poll entirely).
- More translators as new gadgets need them (relative dials/wheels, chords).

## License

MIT — see [LICENSE](LICENSE).
