//! Profiles are DATA, not code. Each file in profiles/ (*.yaml, *.yml, or *.json)
//! deserializes into a `Profile`: how to recognize the device's input nodes, an
//! optional BLE reconnect target, and one or more `translator`s that name a
//! built-in translation algorithm plus its parameters/key-map. Onboarding a
//! device that fits an existing translator needs no recompile — just a new file.
//!
//! A profile may set a single `translator:` or a list of `translators:` (or
//! both). Each device event is offered to them in order; the first that yields a
//! key wins. That lets one device mix algorithms — e.g. a gamepad's stick via
//! `axis` and its face buttons via `keymap`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use evdev::{EventType, InputEvent};
use serde::Deserialize;

use crate::gesture::{name as gesture_name, GestureEngine};
use crate::keys;

/// What a matched input produces: a single key tap, or literal text to type.
/// In a profile map, a value of `type:...` becomes `Type` (a `\n` in it is Enter);
/// anything else is resolved as a key name into `Key`.
#[derive(Clone, Debug)]
pub enum Action {
    Key(u16),
    Type(String),
}

/// Parse one profile map value into an `Action`.
fn parse_binding(s: &str) -> Result<Action> {
    if let Some(text) = s.strip_prefix("type:") {
        Ok(Action::Type(text.to_string()))
    } else {
        let code = keys::code(s).ok_or_else(|| anyhow!("unknown key name '{s}'"))?;
        Ok(Action::Key(code))
    }
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    pub reconnect: Option<Reconnect>,
    #[serde(default)]
    pub translator: Option<TranslatorSpec>,
    #[serde(default)]
    pub translators: Vec<TranslatorSpec>,
}

impl Profile {
    /// All translator specs (the `translators` list then the singular
    /// `translator`), in evaluation order.
    fn specs(&self) -> Vec<&TranslatorSpec> {
        self.translators
            .iter()
            .chain(self.translator.iter())
            .collect()
    }

    pub fn build_translators(&self) -> Result<Vec<Translator>> {
        let specs = self.specs();
        if specs.is_empty() {
            bail!("profile '{}' has no translator", self.name);
        }
        specs.iter().map(|s| s.build()).collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct MatchSpec {
    pub name_exact: Option<String>,
    pub name_prefix: Option<String>,
    pub name_contains: Option<String>,
}

impl MatchSpec {
    pub fn matches(&self, node_name: &str) -> bool {
        if let Some(s) = &self.name_exact {
            return node_name == s;
        }
        if let Some(s) = &self.name_prefix {
            return node_name.starts_with(s);
        }
        if let Some(s) = &self.name_contains {
            return node_name.contains(s);
        }
        false
    }
}

#[derive(Debug, Deserialize)]
pub struct Reconnect {
    pub mac: String,
    pub adapter: Option<String>,
}

fn default_threshold() -> i32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranslatorSpec {
    /// Device fakes a touchpad; classify swipes into up/down/left/right/tap.
    Gesture {
        tap_dist: Option<f64>,
        map: HashMap<String, String>,
    },
    /// Device sends real key codes; remap input key name -> output key name.
    /// Fires on press (value 1).
    Keymap { map: HashMap<String, String> },
    /// Like `keymap`, but fires only on a *clean click*: a key's press (value 1)
    /// immediately followed by its release (value 0), with no autorepeat (value
    /// 2) in between. A held button autorepeats and so never fires — this is how
    /// a quick tap is told apart from a hold on the same key.
    Click { map: HashMap<String, String> },
    /// Device has absolute axes (joystick stick / hat / dpad). Map an axis and a
    /// direction ("ABS_HAT0Y-") to a key, fired once when the axis enters that
    /// direction. `center`/`threshold` define the dead zone (hat: 0/1; an analog
    /// stick 0..255 centered ~127 wants e.g. center 127, threshold 64).
    Axis {
        #[serde(default)]
        center: i32,
        #[serde(default = "default_threshold")]
        threshold: i32,
        map: HashMap<String, String>,
    },
}

impl TranslatorSpec {
    pub fn build(&self) -> Result<Translator> {
        match self {
            TranslatorSpec::Gesture { tap_dist, map } => {
                let mut out = HashMap::new();
                for (g, binding) in map {
                    out.insert(g.to_lowercase(), parse_binding(binding)?);
                }
                Ok(Translator::Gesture {
                    engine: GestureEngine::new(tap_dist.unwrap_or(400.0)),
                    map: out,
                })
            }
            TranslatorSpec::Keymap { map } => {
                let mut out = HashMap::new();
                for (from, to) in map {
                    let fc = keys::code(from)
                        .ok_or_else(|| anyhow!("unknown input key name '{from}'"))?;
                    out.insert(fc, parse_binding(to)?);
                }
                Ok(Translator::Keymap { map: out })
            }
            TranslatorSpec::Click { map } => {
                let mut out = HashMap::new();
                for (from, to) in map {
                    let fc = keys::code(from)
                        .ok_or_else(|| anyhow!("unknown input key name '{from}'"))?;
                    out.insert(fc, parse_binding(to)?);
                }
                Ok(Translator::Click {
                    map: out,
                    armed: HashMap::new(),
                })
            }
            TranslatorSpec::Axis {
                center,
                threshold,
                map,
            } => {
                let mut out: HashMap<(u16, i8), Action> = HashMap::new();
                let mut axes = HashSet::new();
                for (spec, keyname) in map {
                    let (axis_name, sign) = if let Some(a) = spec.strip_suffix('-') {
                        (a, -1i8)
                    } else if let Some(a) = spec.strip_suffix('+') {
                        (a, 1i8)
                    } else {
                        bail!("axis entry '{spec}' must end in '+' or '-'");
                    };
                    let axis = keys::abs_code(axis_name)
                        .ok_or_else(|| anyhow!("unknown axis '{axis_name}'"))?;
                    let key = parse_binding(keyname)?;
                    out.insert((axis, sign), key);
                    axes.insert(axis);
                }
                Ok(Translator::Axis {
                    center: *center,
                    threshold: *threshold,
                    map: out,
                    axes,
                    last: HashMap::new(),
                })
            }
        }
    }
}

/// Runtime translator (holds per-device state). `handle` returns the output key
/// code to inject, if any.
pub enum Translator {
    Gesture {
        engine: GestureEngine,
        map: HashMap<String, Action>,
    },
    Keymap {
        map: HashMap<u16, Action>,
    },
    Click {
        map: HashMap<u16, Action>,
        /// Per-key: saw a press (value 1) that hasn't been disarmed by an
        /// autorepeat. A release while armed is a clean click.
        armed: HashMap<u16, bool>,
    },
    Axis {
        center: i32,
        threshold: i32,
        map: HashMap<(u16, i8), Action>,
        axes: HashSet<u16>,
        last: HashMap<u16, i8>,
    },
}

impl Translator {
    pub fn handle(&mut self, ev: &InputEvent) -> Option<Action> {
        match self {
            Translator::Gesture { engine, map } => {
                let g = engine.feed(ev)?;
                map.get(gesture_name(g)).cloned()
            }
            Translator::Keymap { map } => {
                if ev.event_type() == EventType::KEY && ev.value() == 1 {
                    map.get(&ev.code()).cloned()
                } else {
                    None
                }
            }
            Translator::Click { map, armed } => {
                if ev.event_type() != EventType::KEY {
                    return None;
                }
                let code = ev.code();
                if !map.contains_key(&code) {
                    return None;
                }
                match ev.value() {
                    // press: arm this key
                    1 => {
                        armed.insert(code, true);
                        None
                    }
                    // release: a clean click only if still armed (no autorepeat)
                    0 => {
                        if armed.remove(&code).unwrap_or(false) {
                            map.get(&code).cloned()
                        } else {
                            None
                        }
                    }
                    // autorepeat (2) or anything else: it's a hold — disarm
                    _ => {
                        armed.insert(code, false);
                        None
                    }
                }
            }
            Translator::Axis {
                center,
                threshold,
                map,
                axes,
                last,
            } => {
                if ev.event_type() != EventType::ABSOLUTE {
                    return None;
                }
                let code = ev.code();
                if !axes.contains(&code) {
                    return None;
                }
                let v = ev.value();
                let zone: i8 = if v <= *center - *threshold {
                    -1
                } else if v >= *center + *threshold {
                    1
                } else {
                    0
                };
                let prev = last.get(&code).copied().unwrap_or(0);
                if zone != prev {
                    last.insert(code, zone);
                    if zone != 0 {
                        return map.get(&(code, zone)).cloned();
                    }
                }
                None
            }
        }
    }
}

/// Load every profile file from a directory.
pub fn load_dir(dir: &Path) -> Result<Vec<Profile>> {
    let mut profiles = Vec::new();
    let entries =
        fs::read_dir(dir).with_context(|| format!("reading profiles dir {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let text = match ext {
            "yaml" | "yml" | "json" => {
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
            }
            _ => continue,
        };
        let profile: Profile = if ext == "json" {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        };
        // fail fast on bad key names / translator config
        profile
            .build_translators()
            .with_context(|| format!("in profile {}", path.display()))?;
        profiles.push(profile);
    }
    if profiles.is_empty() {
        bail!("no profiles found in {}", dir.display());
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gesture_profile_and_matches() {
        let yaml = r#"
name: t
match:
  name_prefix: "JX"
translator:
  kind: gesture
  map:
    up: KEY_UP
    tap: KEY_ENTER
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.name, "t");
        assert!(p.match_spec.matches("JX-05"));
        assert!(!p.match_spec.matches("Other"));
        assert_eq!(p.build_translators().unwrap().len(), 1);
    }

    #[test]
    fn unknown_key_name_fails_to_build() {
        let yaml = r#"
name: t
match:
  name_exact: "X"
translator:
  kind: keymap
  map:
    KEY_A: KEY_BOGUS
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(p.build_translators().is_err());
    }

    #[test]
    fn json_profile_also_parses() {
        let json = r#"{"name":"j","match":{"name_contains":"Ring"},
            "translator":{"kind":"keymap","map":{"KEY_VOLUMEUP":"KEY_UP"}}}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(p.match_spec.matches("My Ring"));
        p.build_translators().unwrap();
    }

    #[test]
    fn click_type_binding_fires_on_clean_tap_not_hold() {
        // Mirrors jx05.yaml: the ring's KEY_POWER -> a "continue" macro, but only
        // on a clean press+release (a hold autorepeats and must not fire).
        let yaml = r#"
name: ring
match:
  name_prefix: "JX-05"
translators:
  - kind: click
    map:
      KEY_POWER: "type:continue\n"
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let mut ts = p.build_translators().unwrap();
        let power = keys::code("KEY_POWER").unwrap();
        let down = InputEvent::new(EventType::KEY, power, 1);
        let up = InputEvent::new(EventType::KEY, power, 0);
        let repeat = InputEvent::new(EventType::KEY, power, 2);

        // clean tap: press arms (no fire), release fires the macro
        assert!(ts[0].handle(&down).is_none());
        match ts[0].handle(&up) {
            Some(Action::Type(s)) => assert_eq!(s, "continue\n"),
            other => panic!("expected Type action, got {other:?}"),
        }
        // hold: press, autorepeat, release -> disarmed, so no fire
        assert!(ts[0].handle(&down).is_none());
        assert!(ts[0].handle(&repeat).is_none());
        assert!(ts[0].handle(&up).is_none());
        // a lone release with no preceding press -> nothing
        assert!(ts[0].handle(&up).is_none());
    }

    #[test]
    fn multi_translator_axis_and_keymap() {
        let yaml = r#"
name: pad
match:
  name_prefix: "MOCUTE"
translators:
  - kind: axis
    center: 0
    threshold: 1
    map:
      ABS_HAT0Y-: KEY_UP
      ABS_HAT0Y+: KEY_DOWN
  - kind: keymap
    map:
      BTN_EAST: KEY_ENTER
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let mut ts = p.build_translators().unwrap();
        assert_eq!(ts.len(), 2);

        // hat up (ABS_HAT0Y = -1) -> KEY_UP via the axis translator
        let up = InputEvent::new(
            EventType::ABSOLUTE,
            keys::abs_code("ABS_HAT0Y").unwrap(),
            -1,
        );
        assert!(
            matches!(ts[0].handle(&up), Some(Action::Key(c)) if c == keys::code("KEY_UP").unwrap())
        );
        // returning to center yields nothing
        let center = InputEvent::new(EventType::ABSOLUTE, keys::abs_code("ABS_HAT0Y").unwrap(), 0);
        assert!(ts[0].handle(&center).is_none());
        // BTN_EAST press -> KEY_ENTER via the keymap translator
        let btn = InputEvent::new(EventType::KEY, keys::code("BTN_EAST").unwrap(), 1);
        assert!(
            matches!(ts[1].handle(&btn), Some(Action::Key(c)) if c == keys::code("KEY_ENTER").unwrap())
        );
    }
}
